//! Smart device registry and state management.
//!
//! The smart device registry for the process should be accessed via the [`DEVICES`] global.
//!
//! # Handles
//!
//! Robot programs can access devices via opaque device handles ([`V5_DeviceT`]). RoboScope
//! implements these as pointers into the [`DEVICES`] registry for parity with the VEX V5's
//! behavior. However, handles that are passed to SDK function aren't actually directly
//! dereferenced; instead, the SDK just calculates the handle's [offset in the device array] and
//! continues to access the data they refer to safely.
//!
//! Technically, with this implementation, handles could just be numbers (e.g. `1 as *mut void`
//! would be port 1), but making them pointers ensures that any pointer validity tests run on
//! handles will pass.
//!
//! # Readings, State, and Commands
//!
//! There are 3 smart device artifacts which the SDK uses to control devices: device readings, which
//! are packets from the physics simulator that contain device inputs, device state, which are
//! structs that keep track of the intent of user robot programs, and device commands, which are
//! packets sent to the physics simulator that are generated completely from device state.
//!
//! ```text
//! f(new readings, state) -> new state
//! g(state) -> commands
//! ```
//!
//! The most recent readings for a device (if any) are stored in [`Devices::readings`], and its
//! corresponding state is stored in [`Devices::smart_devices`] - although it may be easier to
//! access them via [`Devices::resolve`]/[`Devices::state_for`] which lets you specify the kind of
//! device readings/state you want with a type parameter.
//!
//! Some devices do not have any state at all: distance sensors cannot be configured and don't need
//! to be told what to do, so they use [`V5DeviceState::None`] (which generates
//! [`DeviceCommand::Empty`] commands). Other devices, like motors, can be configured to return
//! their readings in different units or even commanded to take action in the world, so they use
//! state to remember how they need to act.
//!
//! Devices readings (inputs) are updated at the [physics update period](PHYSICS_UPDATE_PERIOD)
//! from a [task](`crate::sdk::task::vexTasksRun`) callback. Whenever new readings come in, the
//! stored state for each device is compared to what's actually plugged in to that smart port, and
//! if there's a mismatch, the state is re-initialized as the correct type. (`Motor -> Unplugged`
//! wouldn't cause the state to be reinitialized, but `Motor -> Distance` would.) Immediately after
//! that, [`V5DeviceState::command`] is called on each state struct to figure out what to send to
//! the physics simulator.

use std::{
    sync::{Arc, LazyLock},
    time::SystemTime,
};

use derive_more::{From, TryInto};
use itertools::Itertools;
use parking_lot::Mutex;
use roboscope_ipc::{
    Publisher, SMART_DEVICES_COUNT, Sample, SimServices, Subscriber,
    cmd::{DeviceCommand, RobotOutputs},
    snapshot::{DeviceReadings, DeviceSnapshot, DistanceSnapshot, GenericSnapshot, MotorSnapshot},
};
use static_assertions::const_assert_ne;
use tracing::{info, trace};
use vex_sdk::{V5_DeviceT, V5_DeviceType};

use crate::sdk::{generic_serial::GenericSerialState, motor::MotorState};

pub(crate) static TIMESTAMP_EPOCH: LazyLock<SystemTime> = LazyLock::new(SystemTime::now);
pub(crate) static DEVICES_STREAM: Mutex<Option<DevicesStream>> = Mutex::new(None);
pub static DEVICES: Mutex<Devices> = Mutex::new(Devices::new());
pub const NUM_DEVICES_HANDLES: usize = 23;

/// Access to device readings and device I/O.
///
/// This data is exposed via the VEX SDK implementation in the [`crate::sdk`] module.
pub struct Devices {
    pub readings: Option<Sample<DeviceReadings>>,
    /// The state for each smart device.
    pub smart_devices: [V5Device; NUM_DEVICES_HANDLES],
}

impl Devices {
    pub const fn new() -> Self {
        Self {
            readings: None,
            smart_devices: [const { V5Device::new() }; _],
        }
    }

    /// Get the timestamp at which the most recent device packets were created.
    ///
    /// On a real device, this value indicates when CPU0 parsed the device packet, which happens
    /// every 10ms. In the simulator, the physics backend takes the role of CPU0 in providing us
    /// with physics updates every 10ms (see [`PHYSICS_UPDATE_PERIOD`]), so the timestamp indicates
    /// when the physics backend created the packet.
    ///
    /// Just like [`vexSystemTimeGet`], device timestamps are millisecond precision and timestamp
    /// `0` occurs at the start of the robot program.
    pub fn readings_timestamp(&self) -> u32 {
        if let Some(readings) = &self.readings {
            let creation_time = readings.system_time();
            let time_since_epoch = creation_time
                .duration_since(*TIMESTAMP_EPOCH)
                .unwrap_or_default();
            time_since_epoch.as_millis() as u32
        } else {
            0
        }
    }

    /// Apply the given device readings.
    pub fn update_readings(&mut self, sample: Sample<DeviceReadings>) {
        trace!("Committing queued sample");

        // Some devices may need to be re-initialized as a different type if something new was
        // plugged in.
        for (device, snapshot) in self.smart_devices.iter_mut().zip(&sample.snapshots) {
            device.handle_physics_update(snapshot.kind());
        }

        self.readings = Some(sample);
    }

    /// Try to resolve a device identifier to the readings and state for a certain kind of device.
    ///
    /// The given device can either be a port number or a device handle ([`V5_DeviceT`]).
    ///
    /// Returns `None` if the device is the wrong type.
    ///
    /// # Panics
    ///
    /// Panics if the device handle is invalid.
    pub fn resolve<T>(&mut self, device: impl DeviceResolvable) -> Option<(&T, &mut T::State)>
    where
        T: TrackedDevice,
        for<'a> &'a mut T::State: TryFrom<&'a mut V5DeviceState>,
        for<'a> &'a T: TryFrom<&'a DeviceSnapshot>,
    {
        let readings = self.readings.as_ref()?;
        let port = device.to_port(self);

        let snapshot = &readings.snapshots[port];
        let readings = snapshot.try_into().ok()?;

        let Ok(state) = (&mut self.smart_devices[port].state).try_into() else {
            panic!("State should match readings");
        };

        Some((readings, state))
    }

    /// Try to resolve the given device handle to the state for a certain kind of device.
    ///
    /// Returns `None` if the device is the wrong type. Compared to [`Self::resolve`], this method
    /// will succeed even if the device has since been unplugged: state for a specified kind of
    /// device isn't cleared until another kind of device is connected.
    ///
    /// # Panics
    ///
    /// Panics if the device handle is invalid.
    pub fn state_for<T>(&mut self, device: impl DeviceResolvable) -> Option<&mut T>
    where
        for<'a> &'a mut T: TryFrom<&'a mut V5DeviceState>,
    {
        let state = &mut self.smart_devices[device.to_port(self)].state;
        state.try_into().ok()
    }
}

/// Can be resolved to a smart port index.
pub trait DeviceResolvable {
    fn to_port(&self, devices: &Devices) -> usize;
}

/// A direct smart port index.
impl DeviceResolvable for usize {
    fn to_port(&self, _devices: &Devices) -> usize {
        *self
    }
}

/// A device handle.
impl DeviceResolvable for V5_DeviceT {
    /// Convert the device handle to a port number.
    ///
    /// # Panics
    ///
    /// Panics if the device handle is unaligned or out of bounds.
    fn to_port(&self, devices: &Devices) -> usize {
        let offset = self
            .addr()
            .checked_sub(devices.smart_devices.as_ptr().addr())
            .unwrap_or(usize::MAX); // Force an out-of-bounds panic.

        let index = offset / size_of::<V5Device>();
        if index >= devices.smart_devices.len() {
            panic!("Device handle is out of bounds");
        }

        if !offset.is_multiple_of(size_of::<V5Device>()) {
            panic!("Device handle is improperly aligned");
        }

        index
    }
}

pub struct V5Device {
    plugged_type: V5_DeviceType,
    generic_serial: bool,
    state: V5DeviceState,
}
const_assert_ne!(size_of::<V5Device>(), 0); // Pointers to devices must be unique.

impl V5Device {
    pub const fn new() -> Self {
        Self {
            plugged_type: V5_DeviceType::kDeviceTypeNoSensor,
            state: V5DeviceState::None(()),
            generic_serial: false,
        }
    }

    /// Enables generic serial mode.
    ///
    /// Ports that are in generic serial mode will periodically attempt to connect to a serial
    /// peripheral made available via [`SimServices::create_serial_device`]. If successful, the
    /// port's type will switch to [`V5_DeviceType::kDeviceTypeGenericSerial`] and users will be
    /// able to send data to that serial device via the [`crate::sdk::generic_serial`] API.
    pub fn set_generic_serial(&mut self, enabled: bool) {
        if !enabled && self.generic_serial {
            // Reset plugged type and state so the device doesn't show up as generic serial anymore.
            *self = V5Device::new();
            return;
        }
        self.generic_serial = enabled;
    }

    /// Handle new data from the physics sim.
    ///
    /// If this device is determined to hold to wrong kind of state for the given device type, its
    /// [`V5DeviceState`] variant will be changed.
    fn handle_physics_update(&mut self, new_type: V5_DeviceType) {
        // Ports that have been configured as generic serial ignore data from the physics sim until
        // they are [reset](Self::set_generic_serial).
        if self.generic_serial {
            return;
        }

        // If the port is now connected to a concretely different kind of device, reset its state.
        // TODO: Should state be preserved over disconnects?
        if self.plugged_type == new_type || new_type == V5_DeviceType::kDeviceTypeNoSensor {
            return;
        }

        self.plugged_type = new_type;
        self.state = match new_type {
            V5_DeviceType::kDeviceTypeMotorSensor => MotorState::default().into(),
            _ => V5DeviceState::None(()),
        };
    }
}

#[derive(Debug, From, TryInto)]
#[try_into(owned, ref, ref_mut)]
pub enum V5DeviceState {
    None(()),
    Motor(MotorState),
    GenericSerial(GenericSerialState),
}

impl Default for V5DeviceState {
    fn default() -> Self {
        Self::None(())
    }
}

/// An association between a snapshot type and associated device state.
///
/// Allows the [`Devices::resolve`] function to automatically figure out what kind of state to get
/// for a certain type of device.
pub trait TrackedDevice {
    // Tracked devices must have an associated State type which can be obtained via references
    // to `V5DeviceState`. `()` is a valid kind of state, too.
    type State: HasDeviceCommand;
}

impl TrackedDevice for DeviceSnapshot {
    type State = V5DeviceState;
}

impl TrackedDevice for GenericSnapshot {
    type State = ();
}

impl TrackedDevice for DistanceSnapshot {
    type State = ();
}

impl TrackedDevice for MotorSnapshot {
    type State = MotorState;
}

/// A type which can control a smart device.
pub trait HasDeviceCommand {
    /// Get the command that will be sent to the device.
    fn command(&self) -> DeviceCommand;
}

impl HasDeviceCommand for V5DeviceState {
    fn command(&self) -> DeviceCommand {
        match self {
            Self::None(v) => v.command(),
            Self::Motor(motor_state) => motor_state.command(),
            Self::GenericSerial(generic_serial) => generic_serial.command(),
        }
    }
}

impl HasDeviceCommand for () {
    fn command(&self) -> DeviceCommand {
        DeviceCommand::Empty
    }
}

/// Device data updater.
#[derive(Debug)]
pub struct DevicesStream {
    readings: Subscriber<DeviceReadings>,
    outputs: Publisher<RobotOutputs>,
}

impl DevicesStream {
    /// Subscribe to the system device readings channel using the given IPC client.
    pub fn new(ipc: Arc<SimServices>) -> anyhow::Result<Self> {
        let readings = ipc.device_readings()?.subscriber_builder().create()?;
        let outputs = ipc.device_cmds()?.publisher_builder().create()?;

        Ok(Self { readings, outputs })
    }

    /// Get the latest device readings, send commands, and flush buffers.
    pub(crate) fn update(&self) -> anyhow::Result<()> {
        let mut devices = DEVICES.lock();

        // Any device type changes are processed now, which might change the kind of commands we're
        // about to send if a device was re-initialized.
        if let Some(sample) = self.readings.receive()? {
            devices.update_readings(sample);
        }

        let cmds = devices
            .smart_devices
            .iter()
            .map(|dev| dev.state.command())
            .take(SMART_DEVICES_COUNT)
            .collect_array()
            .unwrap();

        self.outputs.send_copy(RobotOutputs(cmds))?;

        // Generic serial needs special handling because its data doesn't come from the physics sim.
        for (port, device) in devices.smart_devices.iter_mut().enumerate() {
            // For smart ports that *want* to be generic serial, but haven't been able to connect to
            // a device yet, try to find a serial device to connect
            if device.generic_serial
                && device.plugged_type != V5_DeviceType::kDeviceTypeGenericSerial
                && let Ok(state) = GenericSerialState::new(port)
            {
                info!(
                    port,
                    "Generic serial connection found a device and is ready to go!"
                );
                device.plugged_type = V5_DeviceType::kDeviceTypeGenericSerial;
                device.state = state.into();
            }
        }

        Ok(())
    }
}
