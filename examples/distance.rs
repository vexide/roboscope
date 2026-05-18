use std::{thread::sleep, time::Duration};

use tracing_subscriber::filter::LevelFilter;
use vex_sdk::*;
use vexide::prelude::Peripherals;

// Note that you need to connect a physics provider to run this example, if you just want something
// simple then `cargo run -p roboscope-ipc --example oscillator`.
#[vexide::main]
async fn main(_p: Peripherals) {
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::WARN)
        .init();
    vex_sdk_desktop::init().unwrap();

    unsafe {
        let sensor = vexDeviceGetByIndex(0);

        loop {
            let distance = vexDeviceDistanceDistanceGet(sensor);
            println!("distance: {distance}");

            vexTasksRun();

            // Intentionally make it slow so you can see the events better.
            sleep(Duration::from_millis(100));
        }
    }
}
