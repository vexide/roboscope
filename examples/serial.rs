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
        .with_max_level(LevelFilter::INFO)
        .init();
    vex_sdk_desktop::init().unwrap();

    unsafe {
        let device = vexDeviceGetByIndex(0);
        vexDeviceGenericSerialEnable(device, 0);

        loop {
            // TODO: implement reads.. for now you can see in the console that the connection worked
            sleep(Duration::from_millis(100)).await;
        }
    }
}
