//! Smart Port Generic Serial Communication

use std::{
    ffi::{OsStr, OsString},
    io,
};

use roboscope_ipc::{SimServices, cmd::DeviceCommand};
use socket2::{Domain, Protocol, SockAddr, Socket, Type};
use tracing::{error, info, warn};
use vex_sdk::V5_DeviceT;

use crate::device::{DEVICES, DeviceResolvable, HasDeviceCommand};

#[derive(Debug)]
pub struct GenericSerialState {
    connection: Socket,
}

impl GenericSerialState {
    /// Connect to a serial device.
    pub fn new(port_idx: usize) -> io::Result<Self> {
        let mut connection = SimServices::connect_to_serial_device(port_idx)?;
        connection.set_nonblocking(true)?;
        Ok(Self { connection })
    }
}

impl HasDeviceCommand for GenericSerialState {
    fn command(&self) -> roboscope_ipc::cmd::DeviceCommand {
        DeviceCommand::Empty
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceGenericSerialEnable(device: V5_DeviceT, options: i32) {
    if options != 0 {
        super::sdk_unimplemented!("vexDeviceGenericSerialEnable(options != 0)");
    }

    let mut ctx = DEVICES.lock();
    let port = device.to_port(&ctx);

    info!(port, "Configuring port as generic serial");
    ctx.smart_devices[port].set_generic_serial(true);
}

#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceGenericSerialBaudrate(device: V5_DeviceT, baudrate: i32) {
    super::sdk_unimplemented!("vexDeviceGenericSerialBaudrate");
}

#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceGenericSerialWriteChar(device: V5_DeviceT, c: u8) -> i32 {
    super::sdk_unimplemented!("vexDeviceGenericSerialWriteChar");
    Default::default()
}

#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceGenericSerialWriteFree(device: V5_DeviceT) -> i32 {
    super::sdk_unimplemented!("vexDeviceGenericSerialWriteFree");
    Default::default()
}

#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceGenericSerialTransmit(
    device: V5_DeviceT,
    buffer: *const u8,
    length: i32,
) -> i32 {
    super::sdk_unimplemented!("vexDeviceGenericSerialTransmit");
    Default::default()
}

#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceGenericSerialReadChar(device: V5_DeviceT) -> i32 {
    super::sdk_unimplemented!("vexDeviceGenericSerialReadChar");
    Default::default()
}

#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceGenericSerialPeekChar(device: V5_DeviceT) -> i32 {
    super::sdk_unimplemented!("vexDeviceGenericSerialPeekChar");
    Default::default()
}

#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceGenericSerialReceiveAvail(device: V5_DeviceT) -> i32 {
    super::sdk_unimplemented!("vexDeviceGenericSerialReceiveAvail");
    Default::default()
}

#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceGenericSerialReceive(
    device: V5_DeviceT,
    buffer: *mut u8,
    length: i32,
) -> i32 {
    super::sdk_unimplemented!("vexDeviceGenericSerialReceive");
    Default::default()
}

#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceGenericSerialFlush(device: V5_DeviceT) {
    super::sdk_unimplemented!("vexDeviceGenericSerialFlush");
}
