//! Controller input viewer.
//!
//! Continually reads both V5 controllers and draws their readings on the Brain screen.
//! The screen is split into two panels showing each joystick's X/Y position, button inputs,
//! connection state, and battery level.

use std::{ffi::CString, thread::sleep, time::Duration};

use tracing_subscriber::filter::LevelFilter;
use vex_sdk::*;
use vexide::prelude::Peripherals;

// Dimensions of a single controller's view.
const PANEL_SIZE: i32 = 240;
const PANEL_HEIGHT: i32 = 240;

const WHITE: u32 = 0xFF_FF_FF;
const BLACK: u32 = 0x00_00_00;
const GREY: u32 = 0x60_60_60;
const GREEN: u32 = 0x00_C8_50;
const RED: u32 = 0xE0_40_40;

/// Radius of the box showing the joystick's position.
const STICK_BOX_SIZE: i32 = 84;
/// Radius of the dot drawn in the joystick's box.
const STICK_DOT_RADIUS: i32 = 5;

#[vexide::main]
async fn main(_p: Peripherals) {
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::WARN)
        .init();
    vex_sdk_desktop::init().unwrap();

    loop {
        unsafe {
            vexDisplayForegroundColor(BLACK);
            vexDisplayRectFill(0, 0, 480, 240);
        }

        draw_controller(0, "Primary", V5_ControllerId::kControllerMaster);
        draw_controller(PANEL_SIZE, "Partner", V5_ControllerId::kControllerPartner);

        unsafe {
            // Divider between the two panels.
            vexDisplayForegroundColor(GREY);
            vexDisplayLineDraw(PANEL_SIZE, 0, PANEL_SIZE, PANEL_HEIGHT);

            vexDisplayRender(true, false);
            vexTasksRun();
        }

        sleep(Duration::from_millis(20));
    }
}

/// Draws one controller's panel, with its left edge at `x0`.
fn draw_controller(x0: i32, name: &str, id: V5_ControllerId) {
    let status = unsafe { vexControllerConnectionStatusGet(id) };
    let connection = match status {
        V5_ControllerStatus::kV5ControllerTethered => "tethered",
        V5_ControllerStatus::kV5ControllerVexnet => "vexnet",
        _ => "offline",
    };

    let mut status_line = format!("{name}  {connection}");

    sdk::text_size(1, 3);
    if status == V5_ControllerStatus::kV5ControllerOffline {
        sdk::text(x0 + 10, 4, RED, &status_line);
        sdk::text_centered(x0 + PANEL_SIZE / 2, PANEL_HEIGHT / 2, GREY, "no controller");
        return;
    }

    let battery = sdk::get(id, V5_ControllerIndex::BatteryCapacity);
    status_line += &format!("  {battery}%");
    sdk::text(x0 + 10, 4, WHITE, &status_line);

    let left = (
        sdk::get(id, V5_ControllerIndex::AnaLeftX),
        sdk::get(id, V5_ControllerIndex::AnaLeftY),
    );
    let right = (
        sdk::get(id, V5_ControllerIndex::AnaRightX),
        sdk::get(id, V5_ControllerIndex::AnaRightY),
    );

    sdk::text_size(1, 4);
    joystick(x0 + 20, 30, left);
    joystick(x0 + 136, 30, right);

    // Shoulder buttons.
    button(
        x0 + 20,
        138,
        44,
        18,
        "L2",
        sdk::pressed(id, V5_ControllerIndex::ButtonL2),
    );
    button(
        x0 + 68,
        138,
        44,
        18,
        "L1",
        sdk::pressed(id, V5_ControllerIndex::ButtonL1),
    );
    button(
        x0 + 128,
        138,
        44,
        18,
        "R1",
        sdk::pressed(id, V5_ControllerIndex::ButtonR1),
    );
    button(
        x0 + 176,
        138,
        44,
        18,
        "R2",
        sdk::pressed(id, V5_ControllerIndex::ButtonR2),
    );

    // D-pad and the ABXY buttons, drawn in a plus shape.
    cluster(
        x0 + 62,
        200,
        id,
        [
            ("^", V5_ControllerIndex::ButtonUp),
            ("v", V5_ControllerIndex::ButtonDown),
            ("<", V5_ControllerIndex::ButtonLeft),
            (">", V5_ControllerIndex::ButtonRight),
        ],
    );
    cluster(
        x0 + 178,
        200,
        id,
        [
            ("X", V5_ControllerIndex::ButtonX),
            ("B", V5_ControllerIndex::ButtonB),
            ("Y", V5_ControllerIndex::ButtonY),
            ("A", V5_ControllerIndex::ButtonA),
        ],
    );

    button(
        x0 + 106,
        191,
        28,
        18,
        "SEL",
        sdk::pressed(id, V5_ControllerIndex::ButtonSEL),
    );
}

/// Draws a joystick inside a bounding box with crosshairs.
///
/// `(x, y)` is the top left corner of the UI element.
fn joystick(x: i32, y: i32, (x_raw, y_raw): (i32, i32)) {
    let center_x = x + STICK_BOX_SIZE / 2;
    let center_y = y + STICK_BOX_SIZE / 2;

    unsafe {
        vexDisplayForegroundColor(GREY);

        // Bounding box and crosshairs.
        vexDisplayRectDraw(x, y, x + STICK_BOX_SIZE, y + STICK_BOX_SIZE);
        vexDisplayLineDraw(x, center_y, x + STICK_BOX_SIZE, center_y);
        vexDisplayLineDraw(center_x, y, center_x, y + STICK_BOX_SIZE);
    }

    // Don't allow the dot to overlap with the bounding box or go past it.
    let travel = STICK_BOX_SIZE / 2 - STICK_DOT_RADIUS - 1;
    let dot_x = center_x + x_raw * travel / 127;
    let dot_y = center_y - y_raw * travel / 127;

    unsafe {
        vexDisplayForegroundColor(GREEN);
        vexDisplayLineDraw(center_x, center_y, dot_x, dot_y);
        vexDisplayCircleFill(dot_x, dot_y, STICK_DOT_RADIUS);
    }

    sdk::text_centered(
        center_x,
        y + STICK_BOX_SIZE + 10,
        WHITE,
        &format!("{x_raw:^4}, {y_raw:^4}"),
    );
}

/// Draws four buttons in a plus shape in up/down/left/right order.
fn cluster(
    center_x: i32,
    center_y: i32,
    id: V5_ControllerId,
    buttons: [(&str, V5_ControllerIndex); 4],
) {
    const SIZE: i32 = 20;
    const OFFSET: i32 = 24;

    let offsets = [(0, -OFFSET), (0, OFFSET), (-OFFSET, 0), (OFFSET, 0)];
    for ((label, index), (dx, dy)) in buttons.into_iter().zip(offsets) {
        button(
            center_x + dx - SIZE / 2,
            center_y + dy - SIZE / 2,
            SIZE,
            SIZE,
            label,
            sdk::pressed(id, index),
        );
    }
}

/// Draws a box which is colored green while a button is held.
fn button(x: i32, y: i32, width: i32, height: i32, label: &str, pressed: bool) {
    unsafe {
        vexDisplayForegroundColor(if pressed { GREEN } else { GREY });
        if pressed {
            vexDisplayRectFill(x, y, x + width, y + height);
        } else {
            vexDisplayRectDraw(x, y, x + width, y + height);
        }
    }

    let color = if pressed { BLACK } else { WHITE };
    sdk::text_centered(x + width / 2, y + height / 2, color, label);
}

/// Safe wrappers for SDK functions.
mod sdk {
    use super::*;

    pub fn get(id: V5_ControllerId, index: V5_ControllerIndex) -> i32 {
        unsafe { vexControllerGet(id, index) }
    }

    pub fn pressed(id: V5_ControllerId, index: V5_ControllerIndex) -> bool {
        get(id, index) != 0
    }

    pub fn text_size(numerator: u32, denominator: u32) {
        unsafe {
            vexDisplayTextSize(numerator, denominator);
        }
    }

    /// Draws text with its top left corner at `(x, y)`.
    pub fn text(x: i32, y: i32, color: u32, string: &str) {
        let string = CString::new(string).unwrap();
        unsafe {
            vexDisplayForegroundColor(color);
            vexDisplayPrintf(x, y, 0, c"%s".as_ptr(), string.as_ptr());
        }
    }

    /// Draws text centered on `(center_x, center_y)`.
    pub fn text_centered(center_x: i32, center_y: i32, color: u32, string: &str) {
        let string = CString::new(string).unwrap();

        unsafe {
            let width = vexDisplayStringWidthGet(string.as_ptr());
            let height = vexDisplayStringHeightGet(string.as_ptr());

            vexDisplayForegroundColor(color);
            vexDisplayPrintf(
                center_x - width / 2,
                center_y - height / 2,
                0,
                c"%s".as_ptr(),
                string.as_ptr(),
            );
        }
    }
}
