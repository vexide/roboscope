# <img src="https://raw.githubusercontent.com/vexide/RoboScope/refs/heads/main/assets/icon-tiny.png" height="120" align="right" alt=""> RoboScope - VEX V5 Simulation

RoboScope is an enhanced desktop SDK for vexide with features like display simulation and the ability to connect to a physics engine. The end-goal for this project is to provide a way for users to design their robot code and train their drivers even when access to a physical robot is limited. This project is developed using only publicly-available or measurable information about the behavior of VEX products.

![Display simulator](./assets/display.gif)

## Getting started

You need to be using a recent nightly version of Rust (see `rust-toolchain.toml`)! vexide v0.8 isn't compatible with Rust versions this new; you need to use the main branch of vexide instead until v0.9 is published.

Add vex-sdk-desktop as a dependency in your Cargo.toml. Make sure you are manually managing what SDK your project uses instead of using vexide's `default-sdk` feature, like this:

```toml
[dependencies.vexide]
git = "https://github.com/vexide/vexide"
features = ["full"]

[target.'cfg(target_os = "vexos")'.dependencies]
vex-sdk-jumptable = { git = "https://github.com/vexide/vex-sdk" }

[target.'cfg(not(target_os = "vexos"))'.dependencies]
vex-sdk-desktop = { git = "https://github.com/lewisfm/vex-simulation" }
```

Then, initialize it from your main function:

```rs
// IMPORTANT: Bring the normal SDK into scope, since we're managing SDKs manually.
#[cfg(target_os = "vexos")]
use vex_sdk_jumptable as _;

#[vexide::main]
async fn main(peripherals: Peripherals) {
    // IMPORTANT: Enable the simulator SDK.
    #[cfg(not(target_os = "vexos"))]
    vex_sdk_desktop::init().expect("Simulator should initialize");

    // ...Whatever else your program normally does...
}
```

Then `cargo run`.

If you would like to see display output, install the display viewer:

```sh
cargo install --git https://github.com/lewisfm/vex-simulation roboscope-viewer
```

Then run `roboscope-viewer` while your program is running.

> [!TIP]
> RoboScope logs warnings via `tracing` if something isn't supported. You should enable a logger so
> you see them!
>
> ```rs
> tracing_subscriber::fmt()
>     .with_max_level(LevelFilter::WARN)
>     .init();
> ```

## Configuration

The file `v5sim.toml` is optionally read from the current directory for simulator configuration.
It has this form:

```toml
# All fields are optional.
debug = ["text-buffer"]
header-hidden = true
theme = "light"
battery-capacity = 100.0
suppress-warnings = [
    "sdk-unimplemented",
    "missing-devices",
    "unknown-enum-variants",
]
```

## Projects

- [Brain Simulator] aka `vex-sdk-desktop`: Drop-in replacement SDK library which provides desktop
    implementations of functions normally only available via VEXos V5. This lets you run unmodified
    robot code like an auton selector or drive program without a V5 brain. Simulation events and
    status updates are published via shared memory for other processes to use.
- [Display Viewer]: A fairly simple app that connects to an active brain simulator and shows the
    current image on the brain's display.
- [IPC library] aka `roboscope-ipc`: Packet definition library for publishing and subscribing to
    simulator data. This could be used to implement a custom simulator visualizer or physics engine.

There are also some example projects under `examples/` which you can try simulating (the GIF above
is the `display` example).

[Brain Simulator]: ./simulator
[Display Viewer]: ./viewer
[IPC Library]: ./ipc

## Features

- Accurate display emulation with mouse input and text rendering
- Connect to an [external physics simulator] or display viewer app
- Compatible with vexide programs and libraries

[external physics simulator]: ./docs/tutorial-physics-sim.md

### API support

Supported (requires a viewer app):

- [x] Display API
- [x] Touch API

Supported (bring your own [physics simulator](./docs/tutorial-physics-sim.md)):

- [x] Smart Devices API
- [x] Motors API
- [x] Distance Sensors API

Supported (bring your own [serial device simulator](./ipc/examples/serial-device.rs)):

- [x] Generic Serial (SerialPort) API

Unsupported (returns dummy values only):

- [ ] Other smart device APIs
- [ ] ADI ports and ADI expanders
- [ ] Competition API
- [ ] Filesystem API (consider using `std::fs`)
- [ ] Controller API
- [ ] System API
- [ ] Task API (system tasks OK, user tasks unsupported)

### What's left

- Wiring up the remaining device APIs to an external physics simulator
- Adding an integration with vexide_startup

## Usage

Run an example V5 program:

```sh
cargo run --example display
```

At the same time, you can run one of these programs to dynamically add features to the simulation:

View the display:

```sh
cargo run -p roboscope-viewer -r
```

Start a physics server for a V5 program to use (this minimal example will connect a distance sensor
on port 1 and oscillate it back and forth):

```sh
cargo run -p roboscope-ipc --example oscillator
```

## Troubleshooting

If you get a stack overflow, make sure that your display drawing code is not allocating any large arrays on the stack. The VEX V5 has a very large stack compared to most operating systems.

## License

Licensed under either of

- Apache License, Version 2.0

  ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license

  ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
