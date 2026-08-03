//! V5 Controller

use anyhow::Result;
use parking_lot::Mutex;
use roboscope_ipc::{
    Sample, SimServices, Subscriber,
    snapshot::{ControllerConnection, ControllerInput, ControllerState},
};
use tracing::warn;
pub use vex_sdk::{V5_ControllerId, V5_ControllerIndex, V5_ControllerStatus};

use crate::{config::{Warning, config}, sdk::{warn_unknown_enum, sdk_unimplemented}};

static STREAM: Mutex<Option<ControllerStream>> = Mutex::new(None);

/// Receives controller data from an external source.
pub struct ControllerStream {
    subscriber: Subscriber<ControllerInput>,
    latest_reading: Option<Sample<ControllerInput>>,
}

impl ControllerStream {
    /// Subscribe to controller data from the given IPC stream.
    pub fn new(ipc: &SimServices) -> Result<Self> {
        let subscriber = ipc.controller_input()?.subscriber_builder().create()?;

        Ok(Self {
            subscriber,
            latest_reading: None,
        })
    }

    /// Receives the latest sample of controller data, if available.
    pub fn update(&mut self) -> Result<()> {
        if let Some(sample) = self.subscriber.receive()? {
            self.latest_reading = Some(sample);
        }
        Ok(())
    }

    /// Gets the most recently received data for the given controller.
    ///
    /// Returns None if no samples have been received yet for that controller. This function
    /// does not attempt to receive new data over IPC.
    pub fn get(&self, id: V5_ControllerId) -> Option<&ControllerState> {
        let sample = self.latest_reading.as_deref()?;

        match id {
            V5_ControllerId::kControllerMaster => Some(&sample.primary),
            V5_ControllerId::kControllerPartner => Some(&sample.partner),
            _ => {
                warn_unknown_enum::<V5_ControllerId>(id.0);
                None
            }
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn vexControllerGet(id: V5_ControllerId, index: V5_ControllerIndex) -> i32 {
    if let Some(stream) = &*STREAM.lock()
        && let Some(state) = stream.get(id)
    {
        if !state.connected() {
            warn_disconnected(id == V5_ControllerId::kControllerPartner);
            return 0;
        }

        match index {
            V5_ControllerIndex::AnaLeftX => state.left_stick.x_raw as i32,
            V5_ControllerIndex::AnaLeftY => state.left_stick.y_raw as i32,
            V5_ControllerIndex::AnaRightX => state.right_stick.x_raw as i32,
            V5_ControllerIndex::AnaRightY => state.right_stick.y_raw as i32,
            V5_ControllerIndex::ButtonL1 => state.button_l1 as i32,
            V5_ControllerIndex::ButtonL2 => state.button_l2 as i32,
            V5_ControllerIndex::ButtonR1 => state.button_r1 as i32,
            V5_ControllerIndex::ButtonR2 => state.button_r2 as i32,
            V5_ControllerIndex::ButtonUp => state.button_up as i32,
            V5_ControllerIndex::ButtonDown => state.button_down as i32,
            V5_ControllerIndex::ButtonLeft => state.button_left as i32,
            V5_ControllerIndex::ButtonRight => state.button_right as i32,
            V5_ControllerIndex::ButtonX => state.button_x as i32,
            V5_ControllerIndex::ButtonB => state.button_b as i32,
            V5_ControllerIndex::ButtonY => state.button_y as i32,
            V5_ControllerIndex::ButtonA => state.button_a as i32,
            V5_ControllerIndex::ButtonSEL => state.button_power as i32,
            V5_ControllerIndex::BatteryLevel => state.battery_level as i32,
            // TODO: This seems to return a bitfield of all buttons. Still TBD
            // on what bits correspond to what flag.
            V5_ControllerIndex::ButtonAll =>  {
                sdk_unimplemented!("V5_ControllerIndex::ButtonAll");
                0
            },
            // TODO: Also TBD on what this is.
            V5_ControllerIndex::Flags => {
                sdk_unimplemented!("V5_ControllerIndex::Flags");
                0
            },
            V5_ControllerIndex::BatteryCapacity => state.battery_capacity as i32,
            _ => {
                warn_unknown_enum::<V5_ControllerIndex>(index.0);
                0
            }
        }
    } else {
        0
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn vexControllerConnectionStatusGet(
    id: V5_ControllerId,
) -> V5_ControllerStatus {
    if let Some(stream) = &*STREAM.lock()
        && let Some(state) = stream.get(id)
    {
        match state.connection {
            ControllerConnection::Offline => V5_ControllerStatus::kV5ControllerOffline,
            ControllerConnection::Tethered => V5_ControllerStatus::kV5ControllerTethered,
            ControllerConnection::Vexnet => V5_ControllerStatus::kV5ControllerVexnet,
        }
    } else {
        V5_ControllerStatus::kV5ControllerOffline
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn vexControllerTextSet(id: u32, line: u32, col: u32, buf: *const u8) -> u32 {
    super::sdk_unimplemented!("vexControllerTextSet");
    Default::default()
}

#[track_caller]
fn warn_disconnected(is_partner: bool) {
    if config()
        .suppress_warnings
        .contains(&Warning::MissingDevices)
    {
        return;
    }

    let controller = if is_partner {
        "Partner"
    } else {
        "Primary"
    };

    warn!(controller, "Tried to read data from a disconnected controller");
}
