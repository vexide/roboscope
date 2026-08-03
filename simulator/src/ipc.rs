use std::{mem::MaybeUninit, sync::Arc, thread};

use roboscope_ipc::{
    Config, SimServices,
    display::{DISPLAY_UPDATE_PERIOD, DisplayFrame},
    error::SimResult,
};
use tracing::trace;

use crate::{
    device::{DEVICES_STREAM, DevicesStream},
    display::{DISPLAY, FRAME_FINISHED},
    sdk::{
        controller::{ControllerStream, STREAM as CONTROLLER_STREAM},
        touch::TOUCH_SUBSCRIBER,
    },
};

pub fn start(name: &str) -> anyhow::Result<()> {
    tracing::debug!("Starting IPC simulator frontend");
    DISPLAY.lock().set_program_name(name);

    let ipc = Arc::new(SimServices::join(
        Some("vex-sdk-desktop"),
        &Config::default(),
    )?);

    *DEVICES_STREAM.lock() = Some(DevicesStream::new(ipc.clone())?);
    *CONTROLLER_STREAM.lock() = Some(ControllerStream::new(&ipc)?);
    *TOUCH_SUBSCRIBER.lock() = Some(
        ipc.display_input()
            .unwrap()
            .subscriber_builder()
            .create()
            .unwrap(),
    );

    thread::Builder::new()
        .name("Simulator Background Thread".into())
        .spawn(move || {
            if let Err(err) = ipc_thread(ipc) {
                tracing::error!(%err, "Sim background thread failed");
            }

            // Either the user has exited or IPC has failed, so shut down the other IPC handles.
            // TODO: Does this crash subscribers?
            *DEVICES_STREAM.lock() = None;
            *CONTROLLER_STREAM.lock() = None;
            *TOUCH_SUBSCRIBER.lock() = None;

            std::process::exit(1);
        })
        .unwrap();

    Ok(())
}

fn ipc_thread(ipc: Arc<SimServices>) -> SimResult<()> {
    let frames = ipc.display_frames()?.publisher_builder().create()?;

    // Break out of the loop when the user presses Ctrl-C or we receive SIGTERM.
    while ipc.node.wait(*DISPLAY_UPDATE_PERIOD).is_ok() {
        let mut next_frame = frames.loan_uninit()?;

        publish_frame(next_frame.payload_mut());

        // SAFETY: init'd by renderer
        let sample = unsafe { next_frame.assume_init() };
        sample.send()?;
    }

    tracing::debug!("Shutting down IPC");

    Ok(())
}

/// Renders a frame by copying the current display data into the given buffer, initializing it.
fn publish_frame(frame: &mut MaybeUninit<DisplayFrame>) {
    let mut disp = DISPLAY.lock();
    disp.render();

    trace!("Publishing a frame");

    let frame_ptr = frame.as_mut_ptr();
    // Direct buffer-to-buffer copy prevents a stack overflow here.
    unsafe {
        let source = &raw const disp.buffer;
        let destination = &raw mut (*frame_ptr).buffer;
        source.copy_to(destination, 1);
    }

    FRAME_FINISHED.notify_all();
}
