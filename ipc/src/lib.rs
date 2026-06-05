//! Data transfer layer for Roboscope

use std::path::PathBuf;
use std::{fmt::Debug, io};
use std::mem::MaybeUninit;
use std::sync::LazyLock;
use std::time::Duration;

use derive_more::{From, TryInto};
use iceoryx2::prelude::*;
use socket2::{Domain, SockAddr, Socket, Type};
use tracing::{debug, info};

use crate::{
    display::{DISPLAY_UPDATE_PERIOD, DisplayFrame, DisplayInput},
    error::{RoboscopeIpcError, SimResult},
};

// Aliases for the kind of IPC types we use.
#[cfg(feature = "thread-safe")]
use ipc_threadsafe as ipc;
pub type PubSubFactory<T> =
    iceoryx2::service::port_factory::publish_subscribe::PortFactory<ipc::Service, T, ()>;
pub type Publisher<T> = iceoryx2::port::publisher::Publisher<ipc::Service, T, ()>;
pub type Subscriber<T> = iceoryx2::port::subscriber::Subscriber<ipc::Service, T, ()>;
pub type Sample<T> = iceoryx2::sample::Sample<ipc::Service, T, ()>;
pub use iceoryx2::config::Config;

pub mod cmd;
pub mod display;
pub mod error;
pub mod snapshot;

pub const PHYSICS_UPDATE_PERIOD: Duration = Duration::from_millis(10);
pub const SMART_DEVICES_COUNT: usize = 21;

#[derive(Debug)]
pub struct SimServices {
    pub node: Node<ipc::Service>,
}

impl SimServices {
    pub fn join(name: Option<&str>, config: &Config) -> SimResult<Self> {
        let node = NodeBuilder::new().config(config);
        SimServices::custom(name, node)
    }

    pub fn custom(name: Option<&str>, mut node: NodeBuilder) -> SimResult<Self> {
        if let Some(name) = name {
            let fmted_name = format!("roboscope.{name}");
            node = node.name(&NodeName::new(&fmted_name).expect("name valid"));
        }

        Ok(Self {
            node: node.create()?,
        })
    }

    fn pub_sub<T: Debug + ZeroCopySend>(&self, name: &str) -> SimResult<PubSubFactory<T>> {
        let name = ServiceName::new(name).unwrap();
        let service = self
            .node
            .service_builder(&name)
            .publish_subscribe::<T>()
            .history_size(1)
            .open_or_create()?;

        Ok(service)
    }

    pub fn display_frames(&self) -> SimResult<PubSubFactory<DisplayFrame>> {
        self.pub_sub("vexide/roboscope/display_frames")
    }

    pub fn display_input(&self) -> SimResult<PubSubFactory<DisplayInput>> {
        self.pub_sub("vexide/roboscope/display_input")
    }

    pub fn device_cmds(&self) -> SimResult<PubSubFactory<cmd::RobotOutputs>> {
        self.pub_sub("vexide/roboscope/device_cmds")
    }

    pub fn device_readings(&self) -> SimResult<PubSubFactory<snapshot::DeviceReadings>> {
        self.pub_sub("vexide/roboscope/device_readings")
    }

    pub fn publish_device_readings(
        &self,
        mut physics_sim: impl FnMut(Option<&cmd::RobotOutputs>) -> snapshot::DeviceReadings,
    ) -> SimResult<()> {
        let robot_subscriber = self.device_cmds()?.subscriber_builder().create()?;
        let captures = self.device_readings()?.publisher_builder().create()?;

        while self.node.wait(PHYSICS_UPDATE_PERIOD).is_ok() {
            let robot_outputs = robot_subscriber.receive()?;
            let physics_inputs = robot_outputs.as_ref().map(Sample::payload);

            let physics_outputs = captures
                .loan_uninit()?
                .write_payload(physics_sim(physics_inputs));

            physics_outputs.send()?;
        }

        Ok(())
    }

    /// Publish a stream of display frames to the simulator at 60Hz.
    ///
    /// # Safety
    ///
    /// The renderer callback is responsible for initializing the frame passed as its argument.
    pub unsafe fn publish_display(
        &self,
        mut renderer: impl FnMut(&mut MaybeUninit<DisplayFrame>),
    ) -> SimResult<()> {
        let frames = self.display_frames()?.publisher_builder().create()?;

        while self.node.wait(*DISPLAY_UPDATE_PERIOD).is_ok() {
            let mut next_frame = frames.loan_uninit()?;

            renderer(next_frame.payload_mut());

            // SAFETY: init'd by renderer
            let sample = unsafe { next_frame.assume_init() };
            sample.send()?;
        }

        Ok(())
    }

    pub fn stream_display(&self, mut cb: impl FnMut(&DisplayFrame)) -> SimResult<()> {
        let frames = self.display_frames()?.subscriber_builder().create()?;

        while self.node.wait(*DISPLAY_UPDATE_PERIOD).is_ok() {
            if let Some(next_frame) = frames.receive()? {
                cb(&next_frame);
            }
        }

        Ok(())
    }

    fn serial_path(port_idx: usize) -> PathBuf {
        std::env::temp_dir().join(format!("roboscope-serialport{port_idx}.sock"))
    }

    /// Create a server which listens for connections from robot programs on a certain smart port.
    ///
    /// Call [`Socket::accept`] to wait for a robot to connect, then write to or read from the
    /// socket that function returns to communicate with a robot program.
    ///
    /// Robot programs can connect to the server via [`Self::connect_to_serial_device`].
    pub fn create_serial_device(port_idx: usize) -> io::Result<Socket> {
        let path = Self::serial_path(port_idx);
        info!(
            port_idx,
            path = ?path.display(),
            "Starting generic serial UNIX domain socket server"
        );

        // Clear up port from previous run.
        _ = std::fs::remove_file(&path);

        let mut socket = Socket::new(Domain::UNIX, Type::STREAM, None)?;
        socket.bind(&SockAddr::unix(path)?)?;
        socket.listen(128)?;

        Ok(socket)
    }

    /// Connect to a serial device server that is listening for connections on the given smart port.
    ///
    /// You can create a peripheral that's compatible with this function using
    /// [`Self::create_serial_device`].
    ///
    /// This function will connect to the UNIX domain socket at the following path, where `TMP` is
    /// the platform's [temporary directory](std::env::temp_dir) and `N` is the zero-indexed port
    /// number passed to this function:
    /// ```text
    /// TMP/roboscope-serialportN.sock
    /// ```
    ///
    /// This behavior is consistent on Linux, macOS, and Windows. Unix domain sockets were chosen
    /// because they're supported by all three platforms.
    pub fn connect_to_serial_device(port_idx: usize) -> io::Result<Socket> {
        let path = Self::serial_path(port_idx);
        debug!(
            port_idx,
            path = ?path.display(),
            "Connecting to generic serial UNIX domain socket server"
        );

        let mut socket = Socket::new(Domain::UNIX, Type::STREAM, None)?;
        socket.connect(&SockAddr::unix(path)?)?;

        Ok(socket)
    }
}
