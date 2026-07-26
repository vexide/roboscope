#![allow(non_camel_case_types)]
#![allow(non_upper_case_globals)]
#![allow(non_snake_case)]
#![allow(unused)]

pub mod abs_enc;
pub mod adi;
pub mod ai_vision;
pub mod arm;
pub mod battery;
pub mod competition;
pub mod controller;
pub mod device;
pub mod display;
pub mod distance;
pub mod file;
pub mod generic_radio;
pub mod generic_serial;
pub mod gps;
pub mod imu;
pub mod led;
pub mod light_tower;
pub mod magnet;
pub mod motor;
pub mod optical;
pub mod pneumatic;
pub mod range;
pub mod serial;
pub mod system;
pub mod task;
pub mod touch;
pub mod vision;

use std::{
    any::type_name,
    collections::HashSet,
    ffi::{VaList, c_char},
    sync::LazyLock,
};

use parking_lot::Mutex;
use tracing::warn;
use vex_sdk::{V5_DeviceT, V5_DeviceType};

use crate::{
    config::{Warning, config},
    device::{DEVICES, DeviceResolvable, Devices},
};

unsafe extern "C" {
    unsafe fn vsnprintf(
        buffer: *mut c_char,
        bufsz: usize,
        format: *const c_char,
        vlist: VaList<'_>,
    );
}

static WARN_ONCE: LazyLock<Mutex<HashSet<&'static str>>> = LazyLock::new(Mutex::default);

macro_rules! sdk_unimplemented {
    ($name:literal) => {{
        use $crate::config::{config, Warning};

        let suppressed = config().suppress_warnings.contains(&Warning::SdkUnimplemented);
        if !suppressed && $crate::sdk::WARN_ONCE.lock().insert($name) {
            ::tracing::warn!(target: "sdk", name = %$name, "[TODO] not implemented");
        }
    }};
}
use sdk_unimplemented;

macro_rules! warn_once {
    ($name:literal, $($msg:tt)+) => {
        if $crate::sdk::WARN_ONCE.lock().insert($name) {
            ::tracing::warn!($($msg)+);
        }
    };
}
use warn_once;

/// Warn that a missing device was accessed.
#[track_caller]
fn warn_unplugged(devices: &Devices, device: V5_DeviceT, expected: V5_DeviceType) {
    if !config()
        .suppress_warnings
        .contains(&Warning::MissingDevices)
    {
        let port = device.to_port(devices);
        warn!(expected = %device_name(expected), port, "Tried to control a missing smart device");
    }
}

#[track_caller]
fn warn_unknown_enum<T>(value: impl Into<u32>) {
    if !config()
        .suppress_warnings
        .contains(&Warning::UnknownEnumVariants)
    {
        let value = value.into();
        let enum_name = type_name::<T>();

        warn!(value, enum_name, "Unknown enum variant");
    }
}

fn device_name(kind: V5_DeviceType) -> &'static str {
    match kind {
        V5_DeviceType::kDeviceTypeNoSensor => "Unplugged",
        V5_DeviceType::kDeviceTypeMotorSensor => "Motor",
        V5_DeviceType::kDeviceTypeLedSensor => "Led",
        V5_DeviceType::kDeviceTypeAbsEncSensor => "AbsEnc",
        V5_DeviceType::kDeviceTypeCrMotorSensor => "CrMotor",
        V5_DeviceType::kDeviceTypeImuSensor => "Imu",
        V5_DeviceType::kDeviceTypeDistanceSensor => "Distance",
        V5_DeviceType::kDeviceTypeRadioSensor => "Radio",
        V5_DeviceType::kDeviceTypeTetherSensor => "Tether",
        V5_DeviceType::kDeviceTypeBrainSensor => "Brain",
        V5_DeviceType::kDeviceTypeVisionSensor => "Vision",
        V5_DeviceType::kDeviceTypeAdiSensor => "Adi",
        V5_DeviceType::kDeviceTypeRes1Sensor => "Res1",
        V5_DeviceType::kDeviceTypeRes2Sensor => "Res2",
        V5_DeviceType::kDeviceTypeRes3Sensor => "Res3",
        V5_DeviceType::kDeviceTypeOpticalSensor => "Optical",
        V5_DeviceType::kDeviceTypeMagnetSensor => "Magnet",
        V5_DeviceType::kDeviceTypeGpsSensor => "Gps",
        V5_DeviceType::kDeviceTypeAicameraSensor => "Aicamera",
        V5_DeviceType::kDeviceTypeLightTowerSensor => "LightTower",
        V5_DeviceType::kDeviceTypeArmDevice => "Arm",
        V5_DeviceType::kDeviceTypeAiVisionSensor => "AiVision",
        V5_DeviceType::kDeviceTypePneumaticSensor => "Pneumatic",
        V5_DeviceType::kDeviceTypeBumperSensor => "Bumper",
        V5_DeviceType::kDeviceTypeGyroSensor => "Gyro",
        V5_DeviceType::kDeviceTypeSonarSensor => "Sonar",
        V5_DeviceType::kDeviceTypeGenericSensor => "Generic",
        V5_DeviceType::kDeviceTypeGenericSerial => "GenericSerial",
        _ => "Undefined",
    }
}

fn warn_invalid_enum() {}
