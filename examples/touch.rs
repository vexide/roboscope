use std::{mem::MaybeUninit, thread::sleep, time::Duration};
use tracing_subscriber::filter::LevelFilter;
use vex_sdk::*;
use vexide::prelude::Peripherals;

#[vexide::main]
async fn main(_p: Peripherals) {
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::WARN)
        .init();
    vex_sdk_desktop::init().unwrap();

    unsafe {
        loop {
            let mut touch_status = MaybeUninit::uninit();
            vexTouchDataGet(touch_status.as_mut_ptr());
            let touch_status = touch_status.assume_init();

            vexDisplayErase();
            vexDisplayForegroundColor(match touch_status.lastEvent {
                V5_TouchEvent::kTouchEventPress => 0xFF_FF_FF,
                V5_TouchEvent::kTouchEventPressAuto => 0x00_FF_00,
                _ => 0xFF_00_FF,
            });
            vexDisplayCircleDraw(
                touch_status.lastXpos as i32,
                touch_status.lastYpos as i32,
                10,
            );

            vexDisplayRender(true, false);
            vexTasksRun();

            // Intentionally make it slow so you can see the events better.
            sleep(Duration::from_millis(100));
        }
    }
}
