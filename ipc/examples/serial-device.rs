//! Generic serial peripheral example.
//!
//! Publishes a device on port 0, waits for a robot to connect, then writes over the serial port.

use std::{io::Write, thread::sleep, time::Duration};

use roboscope_ipc::{SimServices, error::SimResult};
use tracing::Level;

fn main() -> SimResult<()> {
    tracing_subscriber::fmt().with_max_level(Level::INFO).init();

    let device = SimServices::create_serial_device(0)?;
    println!("Waiting for robot");

    let (mut robot, _) = device.accept()?;
    println!("Connection established");

    loop {
        println!("Writing...");
        robot.write_all(b"Hello, robot\n")?;
        sleep(Duration::from_secs(1));
    }
}
