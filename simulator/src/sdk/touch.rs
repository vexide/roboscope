//! Brain Screen Touchscreen

use std::sync::LazyLock;

use parking_lot::Mutex;
use roboscope_ipc::{Subscriber, display::DisplayInput};
pub use vex_sdk::{V5_TouchEvent, V5_TouchStatus};

use crate::{
    canvas::{HEADER_HEIGHT, Point},
    display::DISPLAY,
};

#[unsafe(no_mangle)]
pub extern "system" fn vexTouchUserCallbackSet(callback: extern "C" fn(V5_TouchEvent, i32, i32)) {
    super::sdk_unimplemented!("vexTouchUserCallbackSet");
}

/// Get the status of the brain's touchscreen.
///
/// # Safety
///
/// `status` must be valid for writes.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn vexTouchDataGet(status: *mut V5_TouchStatus) {
    update_touch_status(); // TODO: Should this be in a task instead?

    let display = DISPLAY.lock();
    unsafe {
        *status = display.touch();
    }
}

pub(crate) static TOUCH_SUBSCRIBER: Mutex<Option<Subscriber<DisplayInput>>> = Mutex::new(None);
pub(crate) fn update_touch_status() {
    let mut display = DISPLAY.lock();

    if let Some(subscriber) = &*TOUCH_SUBSCRIBER.lock() {
        // It would make more sense to directly update `display.touch` with this data, but some
        // user libraries act as though "1 call to vexTouchDataGet" = "1 touch event". I'm not sure
        // what's really the correct approach, but this approach of simulating that seems to make
        // them work properly.
        while let Some(sample) = subscriber.receive().expect("could receive sample") {
            display.mouse_down = sample.press_count > sample.release_count;
            display.mouse_coords = Point::new(sample.x as i32, sample.y as i32);
        }
    }

    if display.mouse_down {
        if display.touch.lastEvent == V5_TouchEvent::kTouchEventRelease {
            display.touch.lastEvent = V5_TouchEvent::kTouchEventPress;
            display.touch.pressCount += 1;
        } else {
            display.touch.lastEvent = V5_TouchEvent::kTouchEventPressAuto;
        }

        display.touch.lastXpos = display.mouse_coords.x as i16;
        display.touch.lastYpos = (display.mouse_coords.y - HEADER_HEIGHT) as i16;
    } else {
        if display.touch.lastEvent != V5_TouchEvent::kTouchEventRelease {
            display.touch.releaseCount += 1;
        }

        display.touch.lastEvent = V5_TouchEvent::kTouchEventRelease;
    }
}
