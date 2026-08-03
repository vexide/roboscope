//! Display renderer implementation which writes data to a GUI window.

use std::{
    num::NonZeroU32,
    rc::Rc,
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result, anyhow};
use clap::Parser;
use fast_image_resize::{
    ResizeAlg, ResizeOptions, Resizer,
    images::{TypedImage, TypedImageRef},
    pixels::U8x4,
};
use iceoryx2::{
    node::NodeBuilder, port::update_connections::UpdateConnections,
    signal_handling_mode::SignalHandlingMode,
};
use roboscope_ipc::{
    Config, Publisher, SimServices, Subscriber,
    display::{DISPLAY_HEIGHT, DISPLAY_WIDTH, DisplayFrame, DisplayInput, DisplayInputKind},
    snapshot::{ControllerConnection, ControllerInput, ControllerState},
};
use softbuffer::{Context, Surface};
use tracing::{debug, error, trace};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{KeyEvent, MouseButton, StartCause, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, OwnedDisplayHandle},
    keyboard::{KeyCode, PhysicalKey},
    window::{Theme, Window, WindowId},
};

#[cfg(target_os = "macos")]
mod macos;

const WINDOW_SIZE: LogicalSize<f64> = LogicalSize::new(480.0, 272.0);

type DisplayCtx = Context<OwnedDisplayHandle>;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    /// Publish V5 Controller input using the given data source.
    #[arg(long)]
    ctrl: Option<ControllerSource>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum ControllerSource {
    /// Interpret keyboard input as controller inputs.
    Keyboard,
}

enum ViewerEvent {
    Shutdown,
}

fn main() -> Result<()> {
    let args = Cli::parse();
    ViewerApp::start(args)
}

struct ViewerApp {
    sim_display: Option<SimDisplayWindow>,
    context: DisplayCtx,
    last_frame_time: Option<Instant>,
    subscriber: Option<Subscriber<DisplayFrame>>,
    publisher: Option<Publisher<DisplayInput>>,
    controller: Option<ControllerHandler>,
}

impl ViewerApp {
    fn start(args: Cli) -> Result<()> {
        let builder = NodeBuilder::new()
            .config(&Config::default())
            .signal_handling_mode(SignalHandlingMode::Disabled);
        let ipc = SimServices::custom(Some("viewer"), builder)?;

        let subscriber = ipc.display_frames()?.subscriber_builder().create()?;
        let publisher = ipc.display_input()?.publisher_builder().create()?;

        let event_loop = EventLoop::<ViewerEvent>::with_user_event().build().unwrap();
        let proxy = event_loop.create_proxy();
        ctrlc::set_handler(move || {
            let _ = proxy.send_event(ViewerEvent::Shutdown);
        })
        .context("Failed to register Ctrl-C handler")?;

        let display = event_loop.owned_display_handle();
        let mut simulator = ViewerApp::new(display, subscriber, publisher)?;

        if let Some(source) = args.ctrl {
            simulator.controller = Some(ControllerHandler::new(&ipc, source)?);
        }

        event_loop.run_app(&mut simulator)?;

        Ok(())
    }

    fn new(
        display: OwnedDisplayHandle,
        subscriber: Subscriber<DisplayFrame>,
        publisher: Publisher<DisplayInput>,
    ) -> Result<Self> {
        let context = DisplayCtx::new(display)
            .map_err(|e| anyhow!(e.to_string()))
            .context("Failed to create display rendering context")?;

        Ok(Self {
            sim_display: None,
            context,
            last_frame_time: None,
            subscriber: Some(subscriber),
            publisher: Some(publisher),
            controller: None,
        })
    }

    fn schedule_render(&mut self, event_loop: &ActiveEventLoop, last_render: Instant) {
        let frame_period = Duration::from_secs(1) / 60;
        let now = Instant::now();

        let mut next_render = last_render + frame_period;
        if next_render < now {
            next_render = now + frame_period;
        }

        event_loop.set_control_flow(ControlFlow::WaitUntil(next_render));
    }
}

impl ApplicationHandler<ViewerEvent> for ViewerApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.sim_display.is_none() {
            match SimDisplayWindow::open(
                event_loop,
                &self.context,
                self.subscriber.take().unwrap(),
                self.publisher.take().unwrap(),
            ) {
                Ok(sim_display) => self.sim_display = Some(sim_display),
                Err(error) => error!(%error, "Failed to open VEX V5 Display window"),
            }
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: ViewerEvent) {
        if matches!(event, ViewerEvent::Shutdown) {
            event_loop.exit();
        }
    }

    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        match cause {
            StartCause::Init => {
                // Start a timer for rendering the display at 60 fps.
                self.schedule_render(event_loop, Instant::now());
            }
            StartCause::ResumeTimeReached {
                requested_resume, ..
            } => {
                // 60Hz render timer has triggered, so render a frame.
                self.schedule_render(event_loop, requested_resume);

                let now = Instant::now();
                if let Some(last) = self.last_frame_time.replace(now) {
                    trace!(measured_period = ?now - last, "Frame time");
                }

                if let Some(d) = &mut self.sim_display {
                    d.recv_frame();
                }

                // Deliver the retained controller state to programs which started after the viewer.
                if let Some(controller) = &mut self.controller
                    && let Err(error) = controller.publish_history()
                {
                    error!(%error, "Failed to deliver controller input history");
                }
            }
            _ => {}
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if let Some(mut sim_display) = self.sim_display.take()
            && window_id == sim_display.window_id()
        {
            sim_display.handle_event(self, event_loop, event);
            self.sim_display = Some(sim_display);
        }
    }
}

/// A simulated VEX V5 display.
pub struct SimDisplayWindow {
    window: Rc<Window>,
    surface: Surface<OwnedDisplayHandle, Rc<Window>>,
    subscriber: Subscriber<DisplayFrame>,
    publisher: Publisher<DisplayInput>,
    last_frame: Option<Box<DisplayFrame>>,

    scale_factor: f64,
    num_clicks: u32,
    is_mouse_down: bool,
    mouse_coords: [i16; 2],
}

impl SimDisplayWindow {
    pub fn open(
        event_loop: &ActiveEventLoop,
        context: &DisplayCtx,
        subscriber: Subscriber<DisplayFrame>,
        publisher: Publisher<DisplayInput>,
    ) -> Result<Self> {
        debug!("Opening V5 display window");

        #[cfg(target_os = "macos")]
        self::macos::init_app();

        let attrs = Window::default_attributes()
            .with_resizable(false)
            .with_min_inner_size(WINDOW_SIZE)
            .with_inner_size(WINDOW_SIZE)
            .with_theme(Some(Theme::Dark))
            .with_title("VEX V5 Simulator");

        let window = Rc::new(event_loop.create_window(attrs)?);

        #[cfg(target_os = "macos")]
        {
            window.set_resizable(true);
            self::macos::notify_aspect_ratio(&window);
        }

        let surface = Surface::new(context, window.clone())
            .map_err(|e| anyhow!(e.to_string()))
            .context("Failed to create V5 display rendering surface")?;

        Ok(Self {
            surface,
            window,
            subscriber,
            publisher,
            last_frame: None,
            scale_factor: 1.0,
            is_mouse_down: false,
            mouse_coords: [0, 0],
            num_clicks: 0,
        })
    }

    /// Handle an event sent to this window.
    fn handle_event(
        &mut self,
        app: &mut ViewerApp,
        event_loop: &ActiveEventLoop,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                self.redraw().unwrap();
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if button != MouseButton::Left {
                    return;
                }

                let release_count = self.num_clicks;
                self.is_mouse_down = state.is_pressed();
                if self.is_mouse_down {
                    self.num_clicks = self.num_clicks.wrapping_add(1);
                }

                _ = self.publisher.send_copy(DisplayInput {
                    kind: if self.is_mouse_down {
                        DisplayInputKind::Press
                    } else {
                        DisplayInputKind::Release
                    },
                    press_count: self.num_clicks,
                    release_count,
                    x: self.mouse_coords[0],
                    y: self.mouse_coords[1],
                });
            }
            WindowEvent::CursorMoved { position, .. } => {
                let x = position.x * self.scale_factor;
                let y = position.y * self.scale_factor;
                self.mouse_coords = [x as i16, y as i16];

                if self.is_mouse_down {
                    _ = self.publisher.send_copy(DisplayInput {
                        kind: if self.is_mouse_down {
                            DisplayInputKind::Hold
                        } else {
                            DisplayInputKind::Release
                        },
                        press_count: self.num_clicks,
                        release_count: self.num_clicks - 1,
                        x: self.mouse_coords[0],
                        y: self.mouse_coords[1],
                    });
                }
            }
            WindowEvent::Resized(_) => {
                // Tell the window manager that we have a certain aspect ratio set if possible.
                // This makes dragging the left side of the window resize properly instead of
                // just shifting the window to the left.
                #[cfg(target_os = "macos")]
                self::macos::notify_aspect_ratio(&self.window);

                // Maintain the proper aspect ratio.
                let dims = self.window.inner_size();
                let mut fb_dims = dims;

                let current_aspect_ratio = dims.width as f64 / dims.height as f64;
                let desired_aspect_ratio = WINDOW_SIZE.width / WINDOW_SIZE.height;

                if current_aspect_ratio > desired_aspect_ratio {
                    fb_dims.width = (desired_aspect_ratio * dims.height as f64) as u32;
                } else {
                    fb_dims.height = (1.0 / desired_aspect_ratio * dims.width as f64) as u32;
                }

                if dims != fb_dims && !self.window.is_maximized() {
                    _ = self.window.request_inner_size(fb_dims);
                }

                self.scale_factor = WINDOW_SIZE.width / fb_dims.width as f64;

                // Scale the framebuffer to the window.
                self.surface
                    .resize(
                        NonZeroU32::new(fb_dims.width).unwrap(),
                        NonZeroU32::new(fb_dims.height).unwrap(),
                    )
                    .unwrap();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let Some(controller) = &mut app.controller
                    && let Err(error) = controller.keyboard_input(event)
                {
                    error!(?error, "Failed to publish key input");
                }
            }
            _ => {}
        }
    }

    pub fn recv_frame(&mut self) {
        // AFAIK it's not supposed to work this way, but copying the frame out of shared memory via
        // Box allows the viewer to keep accessing it even if the publisher process exits,
        // preventing a segmentation fault.
        if let Some(frame) = self.subscriber.receive().expect("should receive frame") {
            self.last_frame = Some(Box::new(frame.clone()));
            self.window.request_redraw();
        }
    }

    pub fn window_id(&self) -> WindowId {
        self.window.id()
    }

    /// Scale the display's contents to the size of the window, then write them to the framebuffer.
    pub fn redraw(&mut self) -> Result<()> {
        let next_frame = self
            .subscriber
            .receive()?
            .map(|frame| Box::new(frame.clone()));

        let Some(frame) = next_frame.as_ref().or(self.last_frame.as_ref()) else {
            return Ok(());
        };

        let mut window_buffer = self.surface.buffer_mut().unwrap();
        let width = window_buffer.width().get();
        let height = window_buffer.height().get();

        // Scale the contents to the window size so the entire thing is filled.
        // The destination of the scaled image is the framebuffer itself.

        let buffer_pixels: &[U8x4] = bytemuck::must_cast_slice(&frame.buffer);
        let window_pixels: &mut [U8x4] = bytemuck::must_cast_slice_mut(&mut window_buffer);

        let frame_image = TypedImageRef::new(DISPLAY_WIDTH, DISPLAY_HEIGHT, buffer_pixels).unwrap();
        let mut window_image = TypedImage::from_pixels_slice(width, height, window_pixels).unwrap();

        let mut resizer = Resizer::new();
        resizer
            .resize_typed::<U8x4>(
                &frame_image,
                &mut window_image,
                &ResizeOptions::new()
                    .resize_alg(ResizeAlg::Nearest)
                    .use_alpha(false),
            )
            .unwrap();

        // Swap buffers.
        self.window.pre_present_notify();
        window_buffer.present().unwrap();

        if next_frame.is_some() {
            self.last_frame = next_frame;
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Axis {
    LeftX,
    LeftY,
    RightX,
    RightY,
}

struct ControllerHandler {
    publisher: Publisher<ControllerInput>,
    source: ControllerSource,
    states: ControllerInput,
    kbd_is_partner: bool,
    /// The axis directions currently being held, in the order they were pressed.
    held_directions: Vec<(Axis, bool)>,
}

impl ControllerHandler {
    pub fn new(ipc: &SimServices, source: ControllerSource) -> Result<Self> {
        let publisher = ipc.controller_input()?.publisher_builder().create()?;

        let mut handler = Self {
            publisher,
            source,
            states: ControllerInput::default(),
            kbd_is_partner: false,
            held_directions: Vec::new(),
        };

        handler.refresh_connections();
        handler.publish()?;

        Ok(handler)
    }

    /// Marks the controller which is currently being driven as connected, and the other as offline.
    fn refresh_connections(&mut self) {
        let (connected, offline) = if self.kbd_is_partner {
            (&mut self.states.partner, &mut self.states.primary)
        } else {
            (&mut self.states.primary, &mut self.states.partner)
        };

        connected.connection = ControllerConnection::Tethered;
        connected.battery_level = 100;
        connected.battery_capacity = 100;

        *offline = ControllerState::default();
    }

    /// Update the keyboard's active controller to push the joystick in the direction of the most
    /// recently-held directional key.
    fn apply_axis(&mut self, axis: Axis) {
        // Find the most recently held directional key for this axis.
        let held_direction = self
            .held_directions
            .iter()
            .rev()
            .find(|(held, _)| *held == axis)
            .map(|(_, direction)| *direction);

        let value = match held_direction {
            Some(true) => i8::MAX,
            Some(false) => i8::MIN,
            None => 0,
        };

        let state = if self.kbd_is_partner {
            &mut self.states.partner
        } else {
            &mut self.states.primary
        };

        match axis {
            Axis::LeftX => state.left_stick.x_raw = value,
            Axis::LeftY => state.left_stick.y_raw = value,
            Axis::RightX => state.right_stick.x_raw = value,
            Axis::RightY => state.right_stick.y_raw = value,
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
    pub fn keyboard_input(&mut self, event: KeyEvent) -> Result<()> {
        let ControllerSource::Keyboard = self.source;

        if event.repeat {
            return Ok(());
        }
        let PhysicalKey::Code(code) = event.physical_key else {
            return Ok(());
        };

        // Swap between partner and primary controller.
        if code == KeyCode::KeyP && event.state.is_pressed() {
            self.kbd_is_partner = !self.kbd_is_partner;
            // Don't carry over input to the new controller.
            self.held_directions.clear();
            self.refresh_connections();
            return self.publish();
        }

        let state = if self.kbd_is_partner {
            &mut self.states.partner
        } else {
            &mut self.states.primary
        };

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
            *input = event.state.is_pressed();
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
            if event.state.is_pressed() {
                self.held_directions.push(direction);
            }

            self.apply_axis(direction.0);
        }

        self.publish()
    }
}
