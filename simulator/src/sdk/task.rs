//! VEXos Task Scheduler Functions

use core::ffi::{c_char, c_int, c_void};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use vex_sdk::V5_TouchEvent;

use crate::{
    canvas::HEADER_HEIGHT,
    device::{DEVICES, DEVICES_STREAM},
    display::DISPLAY,
    sdk::controller::STREAM as CONTROLLER_STREAM,
};

#[unsafe(no_mangle)]
pub unsafe extern "system" fn vexTaskAdd(
    callback: extern "C" fn() -> c_int,
    interval: c_int,
    label: *const c_char,
) {
    super::sdk_unimplemented!("vexTaskAdd");
}

#[unsafe(no_mangle)]
pub extern "system" fn vexTaskGetCallbackAndId(index: u32, callback_id: *mut c_int) -> *mut c_void {
    super::sdk_unimplemented!("vexTaskGetCallbackAndId");
    core::ptr::null_mut()
}

#[unsafe(no_mangle)]
pub extern "system" fn vexTaskSleep(time: u32) {
    super::sdk_unimplemented!("vexTaskSleep");
}

#[unsafe(no_mangle)]
pub extern "system" fn vexTaskHardwareConcurrency() -> i32 {
    super::sdk_unimplemented!("vexTaskHardwareConcurrency");
    0
}

#[unsafe(no_mangle)]
pub extern "system" fn vexBackgroundProcessing() {
    super::sdk_unimplemented!("vexBackgroundProcessing");
}

struct Task {
    func: fn(),
    interval: Duration,
    last_run: Option<Instant>,
}

impl Task {
    const fn new(func: fn(), interval: Duration) -> Self {
        Self {
            func,
            interval,
            last_run: None,
        }
    }

    fn poll(&mut self, now: Instant) {
        if let Some(last_run) = self.last_run {
            if (last_run + self.interval) < now {
                self.last_run = Some(now);
                (self.func)();
            }
        } else {
            self.last_run = Some(now);
            (self.func)();
        }
    }
}

static TASKS: Mutex<[Task; 2]> = Mutex::new([
    // Should this be a task? I'm not sure if touch data updates automatically.
    // Task::new(update_touch_status, Duration::from_millis(10)),
    Task::new(update_device_readings, Duration::from_millis(10)),
    Task::new(update_controller_input, Duration::from_millis(10)),
]);

pub fn update_device_readings() {
    if let Some(stream) = &*DEVICES_STREAM.lock() {
        let result = stream.update();
        if let Err(error) = result {
            tracing::error!(%error, "Failed to update devices");
        }
    }
}

pub fn update_controller_input() {
    if let Some(stream) = &mut *CONTROLLER_STREAM.lock() {
        let result = stream.update();
        if let Err(error) = result {
            tracing::error!(%error, "Failed to update controller input");
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn vexTasksRun() {
    let mut tasks = TASKS.lock();
    let now = Instant::now();

    for task in tasks.as_mut() {
        task.poll(now);
    }
}
