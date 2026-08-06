//! V5 controller input handling and publishing.

use std::mem;

use anyhow::{Result, anyhow};
use gilrs::{GamepadId, Gilrs};
use iceoryx2::port::update_connections::UpdateConnections;
use roboscope_ipc::{
    Publisher, SimServices,
    snapshot::{ControllerConnection, ControllerInput, ControllerState},
};
use winit::{
    event::KeyEvent,
    keyboard::{KeyCode, PhysicalKey},
};

#[derive(Debug, Clone, Copy, clap::Args)]
pub struct InputSourceConfig {
    /// Allows the keyboard to provide V5 controller input.
    #[clap(long)]
    pub keyboard: bool,
    /// Allows gamepads to provide V5 controller input.
    #[clap(long)]
    pub gamepad: bool,
    /// Uses the keyboard for controller input even if two gamepads are available.
    #[clap(long)]
    pub force_keyboard: bool,
}

impl InputSourceConfig {
    pub fn has_inputs(self) -> bool {
        self.keyboard || self.gamepad
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputSource {
    Keyboard,
    Gamepad(GamepadId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Axis {
    LeftX,
    LeftY,
    RightX,
    RightY,
}

#[derive(Debug)]
pub struct ControllerHandler {
    publisher: Publisher<ControllerInput>,
    states: ControllerInput,
    sources: [Option<InputSource>; 2],

    keyboard: Option<KeyboardState>,
    gilrs: Option<Gilrs>,
}

impl ControllerHandler {
    pub fn new(ipc: &SimServices, config: InputSourceConfig) -> Result<Self> {
        let publisher = ipc.controller_input()?.publisher_builder().create()?;

        let gamepad_enabled = config.gamepad;
        let gilrs = gamepad_enabled
            .then(Gilrs::new)
            .transpose()
            .map_err(|err| anyhow!("gilrs init failed: {err}"))?;

        let mut sources = [None; 2];

        if let Some(gilrs) = &gilrs {
            // Assign each gamepad to a V5 Controller slot.
            for (id, _) in gilrs.gamepads() {
                if sources[0].is_none() {
                    sources[0] = Some(InputSource::Gamepad(id));
                } else if sources[1].is_none() {
                    sources[1] = Some(InputSource::Gamepad(id));
                }
            }
        }

        if config.keyboard {
            if sources[0].is_none() {
                sources[0] = Some(InputSource::Keyboard);
            } else if sources[1].is_none() || config.force_keyboard {
                sources[1] = Some(InputSource::Keyboard);
            }
        }

        let mut handler = Self {
            publisher,
            keyboard: config.keyboard.then(KeyboardState::default),
            gilrs,
            sources,
            states: ControllerInput::default(),
        };

        handler.refresh_connections();
        handler.publish()?;

        Ok(handler)
    }

    /// Swap primary and partner controllers.
    fn swap(&mut self) {
        self.sources.swap(0, 1);
        mem::swap(&mut self.states.primary, &mut self.states.partner);
    }

    /// Marks controllers which currently have input sources as connected and others as
    /// disconnected.
    pub fn refresh_connections(&mut self) {
        let controllers = [
            (&mut self.states.primary, self.sources[0]),
            (&mut self.states.partner, self.sources[1]),
        ];

        for (state, source) in controllers {
            if source.is_some() {
                state.connection = ControllerConnection::Tethered;
                state.battery_level = 100;
                state.battery_capacity = 100;
            } else {
                *state = ControllerState::default();
            }
        }
    }

    /// Publishes the current state of both controllers.
    pub fn publish(&mut self) -> Result<()> {
        self.publisher.send_copy(self.states)?;
        Ok(())
    }

    /// Publish controller packet history to any new subscribers, without sending new data over IPC.
    pub fn publish_history(&mut self) -> Result<()> {
        self.publisher.update_connections()?;
        Ok(())
    }

    /// Receives new input from the keyboard and publishes any changes to controller state.
    pub fn handle_keyboard(&mut self, event: KeyEvent) -> Result<()> {
        if event.repeat {
            return Ok(());
        }
        let PhysicalKey::Code(code) = event.physical_key else {
            return Ok(());
        };

        // Swap partner and primary controller. This keybind should be available even if
        // the keyboard isn't being used for controller input.
        if code == KeyCode::KeyP && event.state.is_pressed() {
            self.swap();
            return self.publish();
        }

        let Some(keyboard) = &mut self.keyboard else {
            // Keyboard input not enabled.
            return Ok(());
        };

        let controllers = [
            (&mut self.states.primary, self.sources[0]),
            (&mut self.states.partner, self.sources[1]),
        ];

        let mut changes = false;
        for (state, source) in controllers {
            if source == Some(InputSource::Keyboard) {
                changes |= keyboard.update(state, code, event.state.is_pressed());
            }
        }

        if changes {
            self.publish()?;
        }

        Ok(())
    }

    /// Receives new input from connected gamepads and publishes any changes.
    pub fn handle_gamepad(&mut self) -> Result<()> {
        let Some(mut gilrs) = self.gilrs.take() else {
            return Ok(());
        };

        let mut changes = false;

        while let Some(event) = gilrs.next_event() {
            if event.event == gilrs::EventType::Connected {
                changes |= self.connect_gamepad(event.id);
                continue;
            }

            if event.event == gilrs::EventType::Disconnected {
                changes |= self.disconnect_gamepad(event.id);
                continue;
            }

            let mut controllers = [
                (&mut self.states.primary, self.sources[0]),
                (&mut self.states.partner, self.sources[1]),
            ];

            for (state, source) in controllers.iter_mut() {
                if *source == Some(InputSource::Gamepad(event.id)) {
                    changes |= Self::update_gamepad(state, event.event);
                }
            }
        }

        self.gilrs = Some(gilrs);

        if changes {
            self.refresh_connections();
            self.publish()?;
        }

        Ok(())
    }

    /// Add the given gamepad as an input source, if one is needed.
    ///
    /// Returns whether any changes were made.
    fn connect_gamepad(&mut self, id: GamepadId) -> bool {
        if self.sources.contains(&Some(InputSource::Gamepad(id))) {
            return false;
        }

        for source in &mut self.sources {
            if source.is_none() {
                *source = Some(InputSource::Gamepad(id));
                return true;
            }
        }
        false
    }

    /// Removes all input sources that use the given gamepad.
    ///
    /// Returns whether any changes were made.
    fn disconnect_gamepad(&mut self, id: GamepadId) -> bool {
        let mut changed = false;

        for source in &mut self.sources {
            if *source == Some(InputSource::Gamepad(id)) {
                changed = true;
                *source = None;
            }
        }

        changed
    }

    /// Updates the given controller state using input from the specified gamepad event.
    ///
    /// Returns whether any changes occurred.
    fn update_gamepad(state: &mut ControllerState, event: gilrs::EventType) -> bool {
        let (button, pressed) = match event {
            gilrs::EventType::ButtonPressed(button, _) => (Some(button), true),
            gilrs::EventType::ButtonReleased(button, _) => (Some(button), false),
            _ => (None, false),
        };

        if let Some(button) = button {
            let digital_in = match button {
                gilrs::Button::South => Some(&mut state.button_b),
                gilrs::Button::East => Some(&mut state.button_a),
                gilrs::Button::North => Some(&mut state.button_x),
                gilrs::Button::West => Some(&mut state.button_y),
                gilrs::Button::LeftTrigger => Some(&mut state.button_l1),
                gilrs::Button::LeftTrigger2 => Some(&mut state.button_l2),
                gilrs::Button::RightTrigger => Some(&mut state.button_r1),
                gilrs::Button::RightTrigger2 => Some(&mut state.button_r2),
                gilrs::Button::Start => Some(&mut state.button_power),
                gilrs::Button::DPadUp => Some(&mut state.button_up),
                gilrs::Button::DPadDown => Some(&mut state.button_down),
                gilrs::Button::DPadLeft => Some(&mut state.button_left),
                gilrs::Button::DPadRight => Some(&mut state.button_right),
                _ => None,
            };

            if let Some(digital_in) = digital_in {
                *digital_in = pressed;
                return true;
            }
        }

        if let gilrs::EventType::AxisChanged(axis, value, _) = event {
            let analog_in = match axis {
                gilrs::Axis::LeftStickX => Some(&mut state.left_stick.x_raw),
                gilrs::Axis::LeftStickY => Some(&mut state.left_stick.y_raw),
                gilrs::Axis::RightStickX => Some(&mut state.right_stick.x_raw),
                gilrs::Axis::RightStickY => Some(&mut state.right_stick.y_raw),
                _ => None,
            };

            if let Some(analog_in) = analog_in {
                let raw_value = value * i8::MAX as f32;
                *analog_in = raw_value as i8;
                return true;
            }
        }

        false
    }
}

#[derive(Debug, Default)]
struct KeyboardState {
    /// The axis directions currently being held, in the order they were pressed.
    held_directions: Vec<(Axis, bool)>,
}

impl KeyboardState {
    /// Updates the given controller state using input from the specified key event.
    ///
    /// Returns whether any changes were made.
    fn update(&mut self, state: &mut ControllerState, code: KeyCode, pressed: bool) -> bool {
        let binary_input = match code {
            KeyCode::ArrowUp => Some(&mut state.button_up),
            KeyCode::ArrowDown => Some(&mut state.button_down),
            KeyCode::ArrowLeft => Some(&mut state.button_left),
            KeyCode::ArrowRight => Some(&mut state.button_right),

            // Unfortunately there is a conflict with WASD here, so just choose some groupings of
            // keys that feel right (Enter + Shift are good for UIs, otherwise corners of keyboard).
            KeyCode::KeyZ | KeyCode::KeyM | KeyCode::Enter => Some(&mut state.button_a),
            KeyCode::KeyX | KeyCode::Comma | KeyCode::ShiftLeft | KeyCode::ShiftRight => {
                Some(&mut state.button_b)
            }
            KeyCode::KeyC | KeyCode::Period => Some(&mut state.button_x),
            KeyCode::KeyV | KeyCode::Slash => Some(&mut state.button_y),

            KeyCode::KeyQ => Some(&mut state.button_l1),
            KeyCode::KeyE => Some(&mut state.button_r1),
            KeyCode::KeyR | KeyCode::KeyU => Some(&mut state.button_l2),
            KeyCode::KeyF | KeyCode::KeyO => Some(&mut state.button_r2),

            KeyCode::Escape => Some(&mut state.button_power),

            _ => None,
        };

        if let Some(input) = binary_input {
            *input = pressed;
            return true;
        }

        let analog_input = match code {
            KeyCode::KeyW => Some((Axis::LeftY, true)),
            KeyCode::KeyA => Some((Axis::LeftX, false)),
            KeyCode::KeyS => Some((Axis::LeftY, false)),
            KeyCode::KeyD => Some((Axis::LeftX, true)),

            KeyCode::KeyI => Some((Axis::RightY, true)),
            KeyCode::KeyJ => Some((Axis::RightX, false)),
            KeyCode::KeyK => Some((Axis::RightY, false)),
            KeyCode::KeyL => Some((Axis::RightX, true)),

            _ => None,
        };

        if let Some(direction) = analog_input {
            self.held_directions.retain(|held| *held != direction);
            if pressed {
                self.held_directions.push(direction);
            }

            self.apply_axis(state, direction.0);
            return true;
        }

        false
    }

    /// Update the given controller to push the joystick in the direction of the most
    /// recently-held directional key.
    fn apply_axis(&mut self, state: &mut ControllerState, axis: Axis) {
        // Find the most recently held directional key for this axis.
        let held_direction = self
            .held_directions
            .iter()
            .rev()
            .find(|(held, _)| *held == axis)
            .map(|(_, direction)| *direction);

        let value = match held_direction {
            Some(true) => i8::MAX,
            Some(false) => -i8::MAX,
            None => 0,
        };

        match axis {
            Axis::LeftX => state.left_stick.x_raw = value,
            Axis::LeftY => state.left_stick.y_raw = value,
            Axis::RightX => state.right_stick.x_raw = value,
            Axis::RightY => state.right_stick.y_raw = value,
        }
    }
}
