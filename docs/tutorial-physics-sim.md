# Tutorial: Physics Sim

To access devices, you need to write or use a physics server for your robot code. This tutorial will show you how to create your own using the `roboscope-ipc` library.

You can add it to a new project with this command:

```sh
cargo add --git https://github.com/vexide/roboscope roboscope-ipc
```

## Connect to Sim Services

RoboScope communicates between processes using several iceoryx2 services which you can access via the `SimServices` struct. Connect your physics simulator program to a running robot program by passing in the name of your physics simulator and a config:

```rs
use roboscope_ipc::{
    Config, SimServices,
    snapshot::{DeviceReadings, DistanceSnapshot},
};

fn main() -> anyhow::Result<()> {
    let sim = SimServices::join(Some("Example Physics Sim"), &Config::default())?;
}
```

## Publish your physics simulator

Next, register a physics simulator callback:

```rs
let mut readings = DeviceReadings::default();
sim.publish_device_readings(|_cmds| {
    readings
})?;
```

Your callback will be polled at 10Hz by any running robot programs to obtain the latest device state. The values you return in `readings` are authoritative; for example, if you add a `DistanceSnapshot` in index 0 (i.e. Port 1), then the robot program will see that there is a distance sensor connected on that port with the readings you've specified.

```rs
let mut readings = DeviceReadings::default();
sim.publish_device_readings(|_cmds| {
    readings.snapshots[0] = DistanceSnapshot {
        distance: 300,
        ..Default::default()
    }
    .into();

    readings
})?;
```

If you want devices' readings to change over time, you just need to return different values from your callback. For example, the following implementation will simulate a distance sensor that oscillates between 0 and 1000:

```rs
let mut readings = DeviceReadings::default();

let mut x = -500.0;
let mut vx = 0.0;

sim.publish_device_readings(|_cmds| {
    vx -= x * 0.0001;
    x += vx;

    readings.snapshots[0] = DistanceSnapshot {
        distance: (x + 500.0) as u32,
        ..Default::default()
    }
    .into();

    readings
})?;
```

## Handle device commands

Robot programs might periodically issue device commands, which you can use to decide what happens in the physics simulation. For example, a program might want to apply PWM control to a motor, which you can keep track of:

```rs
let mut readings = DeviceReadings::default();
let mut pwm = 0;
let mut x = 0;

sim.publish_device_readings(|cmds| {
    if let Some(cmds) = cmds
        && let DeviceCommand::Motor(motor) = &cmds[0]
        && motor.mode == MotorControlMode::Pwm
    {
        pwm = motor.pwm;
    }

    // More voltage output = more movement speed
    x += pwm / 10;
    readings.snapshots[0] = DistanceSnapshot {
        distance: x.unsigned_abs(),
        ..Default::default()
    }
    .into();

    readings
})?;
```

Where possible, RoboScope will abstract away device configuration so that the inputs to your physics simulator are simpler: your physics simulator doesn't need to handle motor reversal, zero positions, or alternative encoder units.
