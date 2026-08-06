//! IPC packets which hold a snapshot of a device's readings.
//!
//! These values are intended to be produced by a physics simulator or real sensor and consumed
//! by robot code or a visualizer.
//!
//! To publish device readings or consume readings published from another process, see
//! [`SimServices::device_readings`](crate::SimServices::device_readings) and
//! [`SimServices::publish_device_readings`](crate::SimServices::publish_device_readings).

use std::time::{Duration, SystemTime};

use bitflags::bitflags;
use derive_more::{From, TryInto};
use iceoryx2::prelude::ZeroCopySend;
use vex_sdk::V5_DeviceType;

use crate::SMART_DEVICES_COUNT;

/// A packet which reports all the available controller input to the device.
#[derive(Debug, Copy, Clone, PartialEq, ZeroCopySend, Default)]
#[repr(C)]
pub struct ControllerInput {
    pub primary: ControllerState,
    pub partner: ControllerState,
}

/// The method by which a controller is connected.
#[derive(Debug, Copy, Clone, PartialEq, ZeroCopySend, Default)]
#[repr(C)]
pub enum ControllerConnection {
    #[default]
    Offline = 0,
    Tethered = 1,
    Vexnet = 2,
}

/// A sample of readings from a single controller.
#[derive(Debug, Copy, Clone, PartialEq, ZeroCopySend, Default)]
#[repr(C)]
pub struct ControllerState {
    pub connection: ControllerConnection,
    // TODO: Find & document the difference between these
    /// Battery level.
    pub battery_level: u16,
    /// Battery capacity from 0 to 100.
    pub battery_capacity: u8,

    /// Left Joystick
    pub left_stick: JoystickState,
    /// Right Joystick
    pub right_stick: JoystickState,

    /// Button A
    pub button_a: bool,
    /// Button B
    pub button_b: bool,
    /// Button X
    pub button_x: bool,
    /// Button Y
    pub button_y: bool,

    /// Button Up
    pub button_up: bool,
    /// Button Down
    pub button_down: bool,
    /// Button Left
    pub button_left: bool,
    /// Button Right
    pub button_right: bool,

    /// Top Left Bumper
    pub button_l1: bool,
    /// Bottom Left Bumper
    pub button_l2: bool,
    /// Top Right Bumper
    pub button_r1: bool,
    /// Bottom Right Bumper
    pub button_r2: bool,

    /// Center Power Button
    pub button_power: bool,
}

impl ControllerState {
    pub const fn connected(self) -> bool {
        !matches!(self.connection, ControllerConnection::Offline)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, ZeroCopySend, Default)]
#[repr(C)]
pub struct JoystickState {
    pub x_raw: i8,
    pub y_raw: i8,
}

/// A packet which reports the most recent readings from all of a V5 brain's smart ports.
#[derive(Debug, Copy, Clone, PartialEq, ZeroCopySend, Default)]
#[repr(C)]
pub struct DeviceReadings {
    /// The captured device state.
    pub snapshots: [DeviceSnapshot; SMART_DEVICES_COUNT],
    /// Duration in milliseconds since [`UNIX_EPOCH`] which these readings are from.
    ///
    /// [`UNIX_EPOCH`]: std::time::SystemTime::UNIX_EPOCH
    pub timestamp: u64,
}

impl DeviceReadings {
    /// Get the time at which the readings were taken.
    pub fn system_time(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_millis(self.timestamp)
    }
}

/// A type-erased reading from a sensor or other device.
#[derive(Debug, Copy, Clone, PartialEq, ZeroCopySend, Default, From, TryInto)]
#[try_into(owned, ref, ref_mut)]
#[repr(C)]
pub enum DeviceSnapshot {
    #[default]
    Empty,
    Generic(GenericSnapshot),
    Distance(DistanceSnapshot),
    Motor(MotorSnapshot),
}

impl DeviceSnapshot {
    pub fn readings_mut<T>(&mut self) -> Option<&mut T>
    where
        for<'a> &'a mut T: TryFrom<&'a mut DeviceSnapshot>,
    {
        self.try_into().ok()
    }

    pub fn readings<T>(&self) -> Option<&T>
    where
        for<'a> &'a T: TryFrom<&'a DeviceSnapshot>,
    {
        self.try_into().ok()
    }

    pub fn kind(&self) -> V5_DeviceType {
        match self {
            DeviceSnapshot::Empty => V5_DeviceType::kDeviceTypeNoSensor,
            DeviceSnapshot::Generic(_) => V5_DeviceType::kDeviceTypeGenericSensor,
            DeviceSnapshot::Distance(_) => V5_DeviceType::kDeviceTypeDistanceSensor,
            DeviceSnapshot::Motor(_) => V5_DeviceType::kDeviceTypeMotorSensor,
        }
    }
}

/// A reading from a Generic device.
///
/// A Generic device is an actual kind of device similar to distance sensors and motors. If you are
/// looking for a type-erased device snapshot, see [`DeviceSnapshot`].
#[derive(Debug, Copy, Clone, PartialEq, ZeroCopySend, Default)]
#[repr(C)]
pub struct GenericSnapshot {
    /// The device's reading.
    pub value: i32,
}

/// A sensor reading from a distance sensor.
#[derive(Debug, Copy, Clone, PartialEq, ZeroCopySend)]
#[repr(C)]
pub struct DistanceSnapshot {
    /// The distance of the object from the sensor, in millimeters.
    ///
    /// Distance values should be in the range `[20, 2000]`. Additionally, a value of 9999 indicates
    /// that no object was found.
    pub distance: u32,
    /// A value indicating the confidence of the reading in the range `[0, 63]`.
    ///
    /// A confidence of 63 is very high and lower values indicate lower confidence. When confidence
    /// is unavailable (which is the case if [`Self::distance`] > 200), a value of 10 is used.
    pub confidence: u32,
    /// The internal status code of the distance sensor.
    ///
    /// Values of `0x82` and `0x86` indicate normal operation, while a status code of `0x00`
    /// indicates that the sensor is still initializing.
    pub status: u32,
    /// A guess at the object's size, in the range [0, 400].
    ///
    /// A value of -1 indicates that no guess is available. According to the PROS documentation,
    /// an 18" &times; 30" grey card will be result in a value of approximately 75 in typical room
    /// lighting.
    pub object_size: i32,
    /// Approach velocity of the object in m/s, with a low-pass filter applied.
    pub object_velocity: f64,
}

impl Default for DistanceSnapshot {
    fn default() -> Self {
        Self {
            distance: 9999,
            confidence: 0,
            status: 0,
            object_size: -1,
            object_velocity: 0.0,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Eq, PartialEq, ZeroCopySend, Default)]
pub struct MotorStatus(pub u32);

#[repr(C)]
#[derive(Debug, Clone, Copy, Eq, PartialEq, ZeroCopySend, Default)]
pub struct MotorFaults(pub u32);

bitflags! {
    impl MotorStatus: u32 {
        /// Failed to communicate with the motor
        const BUSY = 1 << 0;

        // The real values for these are unknown, but it seems as though they are flags.
        const ZERO_POSITION = 1 << 1;
        const ZERO_VELOCITY = 1 << 2;

        // The source may set any bits
        const _ = !0;
    }

    impl MotorFaults: u32 {
        /// The motor's temperature is above its limit.
        const OVER_TEMPERATURE = 0x01;

        /// The motor is over current.
        const OVER_CURRENT = 0x04;

        /// The motor's H-bridge has encountered a fault.
        const DRIVER_FAULT = 0x02;

        /// The motor's H-bridge is over current.
        const DRIVER_OVER_CURRENT = 0x08;

        // The source may set any bits
        const _ = !0;
    }

}

pub const MOTOR_TICKS_PER_ROTATION: usize = 4096;

#[derive(Debug, Copy, Clone, PartialEq, ZeroCopySend, Default, From)]
#[repr(C)]
pub struct MotorSnapshot {
    /// Measured motor encoder tick reading, where 4096 ticks = 1 rotation.
    ///
    /// Gearset is not taken into consideration in this value.
    pub raw_position: i32,
    /// Measured motor encoder tick readings per second, where 4096 ticks = 1 rotation.
    ///
    /// Gearset is not taken into consideration in this value.
    pub raw_velocity: i32,
    /// Various motor status flags.
    pub flags: MotorStatus,
    /// Various motor fault flags.
    pub faults: MotorFaults,
    /// The temperature of the motor in °C, which should have a resolution of 5°C.
    pub temperature: i32,
    /// The current draw of the motor in milliamperes (mA).
    pub current: i32,
    /// The power draw of the motor in Watts.
    pub power: f64,
    /// The torque output of the motor in Nm.
    pub torque: f64,
    /// The efficiency of the motor from 0 to 100.
    pub efficiency: f64,
    /// The voltage which is currently being applied to the motor in millivolts.
    pub applied_voltage: i32,
}
