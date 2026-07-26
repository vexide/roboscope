//! V5 Smart Motor.
//!
//! This module allows you to access simulated V5 smart motors.
//!
//! # Data transformation
//!
//! This SDK module supports several optional transformations to the data received from the motor
//! simulation. They are applied in this order:
//!
//! 1. [Reversal] (the data from the motor is negated; used for position + velocity)
//! 2. [Zeroing] (the data is offset so that a certain position is reported as zero; position only)
//! 3. [Unit adjustment] (the data is reported in a different unit; position only)
//!
//! [Reversal]: vexDeviceMotorReverseFlagSet
//! [Zeroing]: vexDeviceMotorPositionSet
//! [Unit adjustment]: vexDeviceMotorEncoderUnitsSet
//!
//! Note that changing the reverse flag of the motor when the zero point is set will negate the
//! location which is reported as zero.
//!
//! # Gearing configuration
//!
//! When the current unit is degrees or rotations, the configured [gearing] will be used to
//! properly report positions and velocities.
//!
//! [gearing]: vexDeviceMotorGearingSet

use core::ffi::c_double;

use roboscope_ipc::{
    cmd::{DeviceCommand, MotorBrakeMode, MotorCommand, MotorControlMode, MotorGearset},
    snapshot::{DeviceSnapshot, MOTOR_TICKS_PER_ROTATION, MotorFaults, MotorSnapshot, MotorStatus},
};
use tracing::warn;
pub use vex_sdk::{
    V5_DeviceMotorPid, V5MotorBrakeMode, V5MotorControlMode, V5MotorEncoderUnits, V5MotorGearset,
};
use vex_sdk::{V5_DeviceT, V5_DeviceType};

use crate::{
    device::{DEVICES, DeviceResolvable, HasDeviceCommand},
    sdk::{warn_once, warn_unknown_enum, warn_unplugged},
};

#[derive(Debug, Default)]
pub struct MotorState {
    /// The data which is sent to the motor.
    ///
    /// Note that motor reversal, zeroing, and alternative units are all a level of abstraction
    /// above the actual data sent to the motor, so they're not reflected here.
    cmd: MotorCommand,
    /// The gear cartridge the motor has installed; its multiplier is applied to velocities and
    /// positions.
    gearset: MotorGearset,
    /// Indicates whether motor encoder readings are negated before they are used.
    is_reversed: bool,
    /// The unit which position-related SDK functions will report values in
    position_units: PositionUnits,
    /// Position in ticks which should be reported by SDK functions as zero.
    ///
    /// If [`Self::is_reversed`], the zero point is applied *after* the raw encoder readings are
    /// negated, such that changing the reversal status will move the location that's reported as
    /// zero.
    zero_point: i32,
    /// Maximum voltage to allow, or 0 to disable.
    voltage_limit: i32,
}

impl MotorState {
    /// Apply any enabled transformations to the given position value.
    fn process_position(&self, mut position: i32) -> f64 {
        if self.is_reversed {
            position *= -1;
        }
        position -= self.zero_point;
        position as f64 * self.pos_multiplier()
    }

    /// Get the Raw Ticks -> Position conversion ratio.
    ///
    /// Includes gearset conversion (except for ticks output) and units conversion.
    const fn pos_multiplier(&self) -> f64 {
        let rotations_multipler = self.gearset.multiplier() / MOTOR_TICKS_PER_ROTATION as f64;
        match self.position_units {
            PositionUnits::Ticks => 1.0,
            PositionUnits::Rotations => rotations_multipler,
            PositionUnits::Degrees => rotations_multipler * 360.0,
        }
    }
}

impl HasDeviceCommand for MotorState {
    fn command(&self) -> DeviceCommand {
        self.cmd.into()
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum PositionUnits {
    Degrees = 0,
    Rotations,
    #[default]
    Ticks,
}

impl PositionUnits {}

impl TryFrom<V5MotorEncoderUnits> for PositionUnits {
    type Error = ();

    fn try_from(value: V5MotorEncoderUnits) -> Result<Self, Self::Error> {
        Ok(match value {
            V5MotorEncoderUnits::kMotorEncoderDegrees => Self::Degrees,
            V5MotorEncoderUnits::kMotorEncoderRotations => Self::Rotations,
            V5MotorEncoderUnits::kMotorEncoderCounts => Self::Ticks,
            _ => return Err(()),
        })
    }
}

impl From<PositionUnits> for V5MotorEncoderUnits {
    fn from(value: PositionUnits) -> Self {
        V5MotorEncoderUnits(value as u8)
    }
}

/// Enable internal PID velocity control targeting the given velocity in RPM.
#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorVelocitySet(device: V5_DeviceT, velocity: i32) {
    let mut ctx = DEVICES.lock();

    if let Some((readings, state)) = ctx.resolve::<MotorSnapshot>(device) {
        let velocity_tps = velocity as f64 / state.gearset.multiplier();
        state.cmd.mode = MotorControlMode::Velocity;
        // TODO: store user requested velocity separately from actual velocity target
        state.cmd.target_velocity = velocity;
    } else {
        warn_unplugged(&ctx, device, V5_DeviceType::kDeviceTypeMotorSensor);
    }
}

/// Get the motor's target velocity in RPM.
///
/// Returns zero if the device is not a motor.
#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorVelocityGet(device: V5_DeviceT) -> i32 {
    let mut ctx = DEVICES.lock();

    if let Some((readings, state)) = ctx.resolve::<MotorSnapshot>(device) {
        state.cmd.target_velocity
    } else {
        warn_unplugged(&ctx, device, V5_DeviceType::kDeviceTypeMotorSensor);
        0
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorActualVelocityGet(device: V5_DeviceT) -> c_double {
    let mut ctx = DEVICES.lock();

    if let Some((readings, state)) = ctx.resolve::<MotorSnapshot>(device) {
        let ticks_per_second = readings.raw_velocity as f64;
        let rpm = ticks_per_second / MOTOR_TICKS_PER_ROTATION as f64 * 60.0;
        let mut adjusted_rpm = rpm * state.gearset.multiplier();

        if state.is_reversed {
            adjusted_rpm *= -1.0;
        }

        adjusted_rpm
    } else {
        warn_unplugged(&ctx, device, V5_DeviceType::kDeviceTypeMotorSensor);
        0.0
    }
}

/// Get the direction of movement of the motor's measured velocity.
///
/// Returns 1 if it's moving forwards, 0 if it's stationary, and -1 if it's moving backwards.
#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorDirectionGet(device: V5_DeviceT) -> i32 {
    const VELOCITY_DEADBAND_TPS: u32 = 10; // TODO: Below what velocity does this function return 0?

    let mut ctx = DEVICES.lock();
    if let Some((snapshot, state)) = ctx.resolve::<MotorSnapshot>(device) {
        let mut velocity = snapshot.raw_velocity;
        if state.is_reversed {
            velocity *= -1;
        }

        if velocity.abs() < VELOCITY_DEADBAND_TPS as i32 {
            return 0;
        }

        velocity.signum()
    } else {
        warn_unplugged(&ctx, device, V5_DeviceType::kDeviceTypeMotorSensor);
        0
    }
}

/// Set the current method of motor control.
#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorModeSet(device: V5_DeviceT, mode: V5MotorControlMode) {
    let Ok(mode) = mode.try_into() else {
        warn_unknown_enum::<V5MotorControlMode>(mode.0);
        return;
    };

    let mut ctx = DEVICES.lock();
    if let Some(state) = ctx.state_for::<MotorState>(device) {
        state.cmd.mode = mode;
    } else {
        warn_unplugged(&ctx, device, V5_DeviceType::kDeviceTypeMotorSensor);
    }
}

/// Get the motor's current control method, or [`kMotorControlModeUNDEFINED`] if the mode is PWM
/// or the device isn't a motor.
///
/// [`kMotorControlModeUNDEFINED`]: V5MotorControlMode::kMotorControlModeUNDEFINED
#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorModeGet(device: V5_DeviceT) -> V5MotorControlMode {
    let mut ctx = DEVICES.lock();

    if let Some(state) = ctx.state_for::<MotorState>(device) {
        state
            .cmd
            .mode
            .try_into()
            .unwrap_or(V5MotorControlMode::kMotorControlModeUNDEFINED)
    } else {
        warn_unplugged(&ctx, device, V5_DeviceType::kDeviceTypeMotorSensor);
        V5MotorControlMode::kMotorControlModeUNDEFINED
    }
}

/// Switch the motor to the PWM control mode with the specified PWM control percentage (from `-100`
/// to `100`).
///
/// The configured voltage limit is bypassed.
#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorPwmSet(device: V5_DeviceT, pwm: i32) {
    let mut ctx = DEVICES.lock();

    if let Some(state) = ctx.state_for::<MotorState>(device) {
        if state.voltage_limit != 0 {
            warn_once!(
                "pwm-with-voltage-limit",
                "PWM motor control doesn't respect the configured voltage limit",
            );
        }

        let mut actual_pwm = pwm.clamp(-100, 100);
        if state.is_reversed {
            actual_pwm *= -1;
        }

        state.cmd.mode = MotorControlMode::Pwm;
        state.cmd.pwm = actual_pwm;
    } else {
        warn_unplugged(&ctx, device, V5_DeviceType::kDeviceTypeMotorSensor);
    }
}

/// Get the motor's PWM control percentage.
///
/// Returns zero if the device is not a motor.
#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorPwmGet(device: V5_DeviceT) -> i32 {
    let mut ctx = DEVICES.lock();

    if let Some(state) = ctx.state_for::<MotorState>(device) {
        state.cmd.pwm
    } else {
        warn_unplugged(&ctx, device, V5_DeviceType::kDeviceTypeMotorSensor);
        0
    }
}

/// Set the motor's current limit in milliamperes.
#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorCurrentLimitSet(device: V5_DeviceT, limit: i32) {
    let mut ctx = DEVICES.lock();

    if let Some(state) = ctx.state_for::<MotorState>(device) {
        state.cmd.current_limit = limit;
    } else {
        warn_unplugged(&ctx, device, V5_DeviceType::kDeviceTypeMotorSensor);
    }
}

/// Get the motor's current limit in milliamperes.
///
/// Returns zero if the device is not a motor.
#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorCurrentLimitGet(device: V5_DeviceT) -> i32 {
    let mut ctx = DEVICES.lock();

    if let Some(state) = ctx.state_for::<MotorState>(device) {
        state.cmd.current_limit
    } else {
        warn_unplugged(&ctx, device, V5_DeviceType::kDeviceTypeMotorSensor);
        0
    }
}

/// Get the measured electrical current which the motor is pulling in milliamperes.
#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorCurrentGet(device: V5_DeviceT) -> i32 {
    let mut ctx = DEVICES.lock();

    if let Some((snapshot, _)) = ctx.resolve::<MotorSnapshot>(device) {
        snapshot.current
    } else {
        warn_unplugged(&ctx, device, V5_DeviceType::kDeviceTypeMotorSensor);
        0
    }
}

/// Get the measured power which the motor is pulling in Watts.
#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorPowerGet(device: V5_DeviceT) -> c_double {
    let mut ctx = DEVICES.lock();
    if let Some((snapshot, _)) = ctx.resolve::<MotorSnapshot>(device) {
        snapshot.power
    } else {
        warn_unplugged(&ctx, device, V5_DeviceType::kDeviceTypeMotorSensor);
        0.0
    }
}

// Get the measured torque exerted by the motor in Newton meters.
#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorTorqueGet(device: V5_DeviceT) -> c_double {
    let mut ctx = DEVICES.lock();

    if let Some((snapshot, _)) = ctx.resolve::<MotorSnapshot>(device) {
        snapshot.torque
    } else {
        warn_unplugged(&ctx, device, V5_DeviceType::kDeviceTypeMotorSensor);
        0.0
    }
}

// Get the percent efficiency of the motor, from 0 to 100.
#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorEfficiencyGet(device: V5_DeviceT) -> c_double {
    let mut ctx = DEVICES.lock();

    if let Some((snapshot, _)) = ctx.resolve::<MotorSnapshot>(device) {
        snapshot.efficiency
    } else {
        warn_unplugged(&ctx, device, V5_DeviceType::kDeviceTypeMotorSensor);
        0.0
    }
}

// Get the temperature of the motor in Celsius.
#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorTemperatureGet(device: V5_DeviceT) -> c_double {
    let mut ctx = DEVICES.lock();

    if let Some((snapshot, _)) = ctx.resolve::<MotorSnapshot>(device) {
        snapshot.temperature as f64
    } else {
        warn_unplugged(&ctx, device, V5_DeviceType::kDeviceTypeMotorSensor);
        0.0
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorOverTempFlagGet(device: V5_DeviceT) -> bool {
    let mut ctx = DEVICES.lock();

    if let Some((snapshot, _)) = ctx.resolve::<MotorSnapshot>(device) {
        snapshot.faults.contains(MotorFaults::OVER_TEMPERATURE)
    } else {
        warn_unplugged(&ctx, device, V5_DeviceType::kDeviceTypeMotorSensor);
        false
    }
}
#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorCurrentLimitFlagGet(device: V5_DeviceT) -> bool {
    let mut ctx = DEVICES.lock();

    if let Some((snapshot, _)) = ctx.resolve::<MotorSnapshot>(device) {
        snapshot.faults.contains(MotorFaults::OVER_CURRENT)
    } else {
        warn_unplugged(&ctx, device, V5_DeviceType::kDeviceTypeMotorSensor);
        false
    }
}
#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorZeroVelocityFlagGet(device: V5_DeviceT) -> bool {
    let mut ctx = DEVICES.lock();

    if let Some((snapshot, _)) = ctx.resolve::<MotorSnapshot>(device) {
        snapshot.flags.contains(MotorStatus::ZERO_VELOCITY)
    } else {
        warn_unplugged(&ctx, device, V5_DeviceType::kDeviceTypeMotorSensor);
        false
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorZeroPositionFlagGet(device: V5_DeviceT) -> bool {
    let mut ctx = DEVICES.lock();

    if let Some((snapshot, _)) = ctx.resolve::<MotorSnapshot>(device) {
        snapshot.flags.contains(MotorStatus::ZERO_POSITION)
    } else {
        warn_unplugged(&ctx, device, V5_DeviceType::kDeviceTypeMotorSensor);
        false
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorReverseFlagSet(device: V5_DeviceT, reverse: bool) {
    let mut ctx = DEVICES.lock();
    if let Some(state) = ctx.state_for::<MotorState>(device) {
        state.is_reversed = reverse;
    } else {
        warn_unplugged(&ctx, device, V5_DeviceType::kDeviceTypeMotorSensor);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorReverseFlagGet(device: V5_DeviceT) -> bool {
    let mut ctx = DEVICES.lock();
    if let Some(state) = ctx.state_for::<MotorState>(device) {
        state.is_reversed
    } else {
        warn_unplugged(&ctx, device, V5_DeviceType::kDeviceTypeMotorSensor);
        false
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorEncoderUnitsSet(
    device: V5_DeviceT,
    units: V5MotorEncoderUnits,
) {
    let Ok(units) = units.try_into() else {
        warn_unknown_enum::<V5MotorEncoderUnits>(units.0);
        return;
    };

    let mut ctx = DEVICES.lock();
    if let Some(state) = ctx.state_for::<MotorState>(device) {
        state.position_units = units;
    } else {
        warn_unplugged(&ctx, device, V5_DeviceType::kDeviceTypeMotorSensor);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorEncoderUnitsGet(device: V5_DeviceT) -> V5MotorEncoderUnits {
    let mut ctx = DEVICES.lock();
    if let Some(state) = ctx.state_for::<MotorState>(device) {
        state.position_units.into()
    } else {
        warn_unplugged(&ctx, device, V5_DeviceType::kDeviceTypeMotorSensor);
        PositionUnits::default().into()
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorBrakeModeSet(device: V5_DeviceT, mode: V5MotorBrakeMode) {
    let Ok(mode) = mode.try_into() else {
        warn_unknown_enum::<V5MotorBrakeMode>(mode.0);
        return;
    };

    let mut ctx = DEVICES.lock();
    if let Some(state) = ctx.state_for::<MotorState>(device) {
        state.cmd.brake_mode = mode;
    } else {
        warn_unplugged(&ctx, device, V5_DeviceType::kDeviceTypeMotorSensor);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorBrakeModeGet(device: V5_DeviceT) -> V5MotorBrakeMode {
    let mut ctx = DEVICES.lock();
    if let Some(state) = ctx.state_for::<MotorState>(device) {
        state.cmd.brake_mode.into()
    } else {
        warn_unplugged(&ctx, device, V5_DeviceType::kDeviceTypeMotorSensor);
        MotorBrakeMode::default().into()
    }
}

/// Apply an offset to the position.
#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorPositionSet(device: V5_DeviceT, position: c_double) {
    let mut ctx = DEVICES.lock();
    if let Some((snapshot, state)) = ctx.resolve::<MotorSnapshot>(device) {
        let desired_pos_ticks = (position / state.pos_multiplier()) as i32;
        let mut real_pos = snapshot.raw_position;

        // NB. If this flag is later changed, then the offset will be applied in the wrong
        // direction. This is intentional, to match VEX's behavior.
        if state.is_reversed {
            real_pos *= -1;
        }

        state.zero_point = desired_pos_ticks - real_pos;
    } else {
        warn_unplugged(&ctx, device, V5_DeviceType::kDeviceTypeMotorSensor);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorPositionGet(device: V5_DeviceT) -> c_double {
    let mut ctx = DEVICES.lock();
    if let Some((snapshot, state)) = ctx.resolve::<MotorSnapshot>(device) {
        state.process_position(snapshot.raw_position)
    } else {
        warn_unplugged(&ctx, device, V5_DeviceType::kDeviceTypeMotorSensor);
        0.0
    }
}

/// Get the raw position of the motor in raw ticks.
///
/// If the motor is reversed, the returned value will be negated like usual, but other
/// transformations like zeroing and alternative units are not applied.
/// The low-res timestamp at which the position was received will be written to `timestamp`, but
/// only if the device is actually a motor.
///
/// # Safety
///
/// `timestamp` must either be null or valid for writes.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn vexDeviceMotorPositionRawGet(
    device: V5_DeviceT,
    timestamp: *mut u32,
) -> i32 {
    let mut ctx = DEVICES.lock();
    if let Some((snapshot, state)) = ctx.resolve::<MotorSnapshot>(device) {
        let mut raw_position = snapshot.raw_position;
        if state.is_reversed {
            raw_position *= -1;
        }

        if !timestamp.is_null() {
            unsafe {
                *timestamp = ctx.readings_timestamp();
            }
        }

        raw_position
    } else {
        warn_unplugged(&ctx, device, V5_DeviceType::kDeviceTypeMotorSensor);
        0
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorPositionReset(device: V5_DeviceT) {
    let mut ctx = DEVICES.lock();
    if let Some((snapshot, state)) = ctx.resolve::<MotorSnapshot>(device) {
        let mut current_pos = snapshot.raw_position;
        if state.is_reversed {
            current_pos *= -1;
        }

        state.zero_point = current_pos;
    } else {
        warn_unplugged(&ctx, device, V5_DeviceType::kDeviceTypeMotorSensor);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorTargetGet(device: V5_DeviceT) -> c_double {
    let mut ctx = DEVICES.lock();
    if let Some((snapshot, state)) = ctx.resolve::<MotorSnapshot>(device) {
        state.process_position(state.cmd.target_position)
    } else {
        warn_unplugged(&ctx, device, V5_DeviceType::kDeviceTypeMotorSensor);
        0.0
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorServoTargetSet(device: V5_DeviceT, position: c_double) {
    super::sdk_unimplemented!("vexDeviceMotorServoTargetSet");
}

/// Switch the motor to position control, commanding it to move towards the given position at the
/// given velocity in RPM.
///
/// The specified position is absolute (not relative to the current position) and should be
/// given in the same form and units that [`vexDeviceMotorPositionGet`] returns. For example,
/// passing `5` will move the motor such that [`vexDeviceMotorPositionGet`] eventually returns `5`.
#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorAbsoluteTargetSet(
    device: V5_DeviceT,
    position: c_double,
    velocity: i32,
) {
    let mut ctx = DEVICES.lock();
    if let Some((snapshot, state)) = ctx.resolve::<MotorSnapshot>(device) {
        // Un-apply the position transformations in the reverse order to get plain ticks that we
        // can pass to the motor.
        let mut position_ticks = (position / state.pos_multiplier()) as i32 + state.zero_point;
        if state.is_reversed {
            position_ticks *= -1;
        }

        state.cmd.mode = MotorControlMode::Position;
        state.cmd.target_position = position_ticks;
    } else {
        warn_unplugged(&ctx, device, V5_DeviceType::kDeviceTypeMotorSensor);
    }
}

/// Switch the motor to position control, commanding it to move towards the given relative position
/// at the given velocity in RPM.
///
/// The specified position should be given relative to the current position with the same units as
/// are returned by [`vexDeviceMotorPositionGet`]. For example, passing `5` will move the motor an
/// additional `5` units forward.
#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorRelativeTargetSet(
    device: V5_DeviceT,
    position: c_double,
    velocity: i32,
) {
    let mut ctx = DEVICES.lock();
    if let Some((snapshot, state)) = ctx.resolve::<MotorSnapshot>(device) {
        // Un-apply the position transformations in the reverse order to get plain ticks that we
        // can pass to the motor.
        let mut position_ticks = (position / state.pos_multiplier()) as i32 + state.zero_point;
        if state.is_reversed {
            position_ticks *= -1;
        }

        state.cmd.mode = MotorControlMode::Position;
        state.cmd.target_position = position_ticks;
    } else {
        warn_unplugged(&ctx, device, V5_DeviceType::kDeviceTypeMotorSensor);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorFaultsGet(device: V5_DeviceT) -> u32 {
    let mut ctx = DEVICES.lock();
    if let Some((snapshot, _)) = ctx.resolve::<MotorSnapshot>(device) {
        snapshot.faults.0
    } else {
        warn_unplugged(&ctx, device, V5_DeviceType::kDeviceTypeMotorSensor);
        0
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorFlagsGet(device: V5_DeviceT) -> u32 {
    let mut ctx = DEVICES.lock();
    if let Some((snapshot, _)) = ctx.resolve::<MotorSnapshot>(device) {
        snapshot.flags.0
    } else {
        warn_unplugged(&ctx, device, V5_DeviceType::kDeviceTypeMotorSensor);
        0
    }
}

const MAX_VOLTAGE: i32 = 12_000;

/// Switch the motor to PWM control and command it to run at the given voltage (in millivolts).
#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorVoltageSet(device: V5_DeviceT, voltage: i32) {
    let mut ctx = DEVICES.lock();
    if let Some(state) = ctx.state_for::<MotorState>(device) {
        let mut actual_voltage = voltage.clamp(-MAX_VOLTAGE, MAX_VOLTAGE);

        if state.voltage_limit != 0 {
            actual_voltage =
                actual_voltage.clamp(-state.voltage_limit.abs(), state.voltage_limit.abs());
        }
        if state.is_reversed {
            actual_voltage *= -1;
        }

        state.cmd.mode = MotorControlMode::Pwm;
        state.cmd.pwm = actual_voltage;
    } else {
        warn_unplugged(&ctx, device, V5_DeviceType::kDeviceTypeMotorSensor);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorVoltageGet(device: V5_DeviceT) -> i32 {
    let mut ctx = DEVICES.lock();
    if let Some((snapshot, state)) = ctx.resolve::<MotorSnapshot>(device) {
        let mut voltage = snapshot.applied_voltage;
        if state.is_reversed {
            voltage *= -1;
        }
        voltage
    } else {
        warn_unplugged(&ctx, device, V5_DeviceType::kDeviceTypeMotorSensor);
        0
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorGearingSet(device: V5_DeviceT, gearset: V5MotorGearset) {
    let Some(gearset) = MotorGearset::new(gearset) else {
        warn_unknown_enum::<V5MotorGearset>(gearset.0);
        return;
    };

    let mut ctx = DEVICES.lock();
    if let Some(state) = ctx.state_for::<MotorState>(device) {
        state.gearset = gearset;
    } else {
        warn_unplugged(&ctx, device, V5_DeviceType::kDeviceTypeMotorSensor);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorGearingGet(device: V5_DeviceT) -> V5MotorGearset {
    let mut ctx = DEVICES.lock();
    if let Some(state) = ctx.state_for::<MotorState>(device) {
        state.gearset.to_v5()
    } else {
        warn_unplugged(&ctx, device, V5_DeviceType::kDeviceTypeMotorSensor);
        MotorGearset::default().to_v5()
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorVoltageLimitSet(device: V5_DeviceT, limit: i32) {
    let mut ctx = DEVICES.lock();
    if let Some(state) = ctx.state_for::<MotorState>(device) {
        if limit.abs() > MAX_VOLTAGE.abs() {
            warn_once!(
                "voltage-limit-useless",
                limit,
                "Voltage limits larger than {MAX_VOLTAGE} have no effect."
            );
        }

        state.voltage_limit = limit;
    } else {
        warn_unplugged(&ctx, device, V5_DeviceType::kDeviceTypeMotorSensor);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorVoltageLimitGet(device: V5_DeviceT) -> i32 {
    let mut ctx = DEVICES.lock();
    if let Some(state) = ctx.state_for::<MotorState>(device) {
        state.voltage_limit
    } else {
        warn_unplugged(&ctx, device, V5_DeviceType::kDeviceTypeMotorSensor);
        0
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorVelocityUpdate(device: V5_DeviceT, mut velocity: i32) {
    let mut ctx = DEVICES.lock();
    if let Some(state) = ctx.state_for::<MotorState>(device) {
        if state.is_reversed {
            velocity *= -1;
        }

        state.cmd.mode = MotorControlMode::Velocity;
        state.cmd.target_velocity = velocity;
    } else {
        warn_unplugged(&ctx, device, V5_DeviceType::kDeviceTypeMotorSensor);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorPositionPidSet(
    device: V5_DeviceT,
    pid: *mut V5_DeviceMotorPid,
) {
    super::sdk_unimplemented!("vexDeviceMotorPositionPidSet");
}

#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorVelocityPidSet(
    device: V5_DeviceT,
    pid: *mut V5_DeviceMotorPid,
) {
    super::sdk_unimplemented!("vexDeviceMotorVelocityPidSet");
}
#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorExternalProfileSet(
    device: V5_DeviceT,
    position: c_double,
    velocity: i32,
) {
    super::sdk_unimplemented!("vexDeviceMotorExternalProfileSet");
}
