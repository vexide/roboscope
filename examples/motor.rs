//! Motor device example.
//!
//! This example works well with the "simple-motor" physics server example. You should run it at the
//! same time (in a 2nd terminal window) if you'd like to see the motor move:
//!
//! ```sh
//! cd ipc
//! cargo run --example simple-motor
//! ```

use std::time::Duration;
use tracing_subscriber::filter::LevelFilter;
use vex_sdk::*;
use vexide::{prelude::Peripherals, time::sleep};

#[vexide::main]
async fn main(_p: Peripherals) {
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::WARN)
        .init();
    vex_sdk_desktop::init().unwrap();

    unsafe {
        println!("Waiting for a motor to connect on port 1...");
        let mut devices = [V5_DeviceType::kDeviceTypeNoSensor; V5_MAX_DEVICE_PORTS];
        loop {
            vexDeviceGetStatus(devices.as_mut_ptr());
            if devices[0] == V5_DeviceType::kDeviceTypeMotorSensor {
                break;
            }
            sleep(Duration::from_millis(250)).await;
        }

        println!("Running motor at 12V...");

        let motor = vexDeviceGetByIndex(0);
        vexDeviceMotorVoltageSet(motor, 12_000);
        vexDeviceMotorEncoderUnitsSet(motor, V5MotorEncoderUnits::kMotorEncoderCounts);

        loop {
            let encoder_reading = vexDeviceMotorPositionGet(motor);

            println!("Encoder reading: {encoder_reading:2}°");

            // Intentionally make it slow so you can see the events better.
            sleep(Duration::from_millis(100)).await;
        }
    }
}
