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
use std::cmp;

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

#[derive(Debug, Default, Clone, Copy)]
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
}

impl MotorState {
    /// Switch to PWM control with the given percentage of the voltage limit.
    ///
    /// The value is clamped to `[-100, 100]` and negated if the motor is reversed.
    fn set_pwm(&mut self, pwm: i32) {
        let mut actual_pwm = pwm.clamp(-100, 100);
        if self.is_reversed {
            actual_pwm *= -1;
        }

        self.cmd.mode = MotorControlMode::Pwm;
        self.cmd.pwm = actual_pwm;
    }

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
/// The percentage is of the configured [voltage limit](vexDeviceMotorVoltageLimitSet), so with a
/// 6000 mV limit, a PWM of `100` outputs 6000 mV.
#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorPwmSet(device: V5_DeviceT, pwm: i32) {
    let mut ctx = DEVICES.lock();

    if let Some(state) = ctx.state_for::<MotorState>(device) {
        state.set_pwm(pwm);
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
        // `cmd.pwm` is stored with reversal applied, so undo it.
        if state.is_reversed {
            -state.cmd.pwm
        } else {
            state.cmd.pwm
        }
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

/// Switch the motor to PWM control and command it to run at the given voltage (in millivolts).
///
/// The voltage is clamped to the configured [voltage limit](vexDeviceMotorVoltageLimitSet) and
/// then converted to a PWM percentage of that limit, rounding toward zero.
#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorVoltageSet(device: V5_DeviceT, voltage: i32) {
    let mut ctx = DEVICES.lock();
    if let Some(state) = ctx.state_for::<MotorState>(device) {
        // The limit isn't ever zero because vexDeviceMotorVoltageLimitSet clamps it.
        let limit = cmp::min(state.cmd.voltage_limit.abs(), MotorCommand::MAX_VOLTAGE);
        let actual_voltage = voltage.clamp(-limit, limit);
        state.set_pwm(actual_voltage * 100 / limit);
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

/// Set the motor's maximum output voltage in millivolts.
///
/// The limit is clamped to `[1000, 12000]`, so values below 1000 (including zero and negative
/// numbers) set a 1000 mV limit.
#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorVoltageLimitSet(device: V5_DeviceT, limit: i32) {
    let mut ctx = DEVICES.lock();
    if let Some(state) = ctx.state_for::<MotorState>(device) {
        let actual_limit = limit.clamp(MotorCommand::MIN_VOLTAGE_LIMIT, MotorCommand::MAX_VOLTAGE);
        if actual_limit != limit {
            warn_once!(
                "voltage-limit-clamped",
                limit,
                actual_limit,
                "An out-of-range motor voltage limit was clamped to [{}, {}] mV.",
                MotorCommand::MIN_VOLTAGE_LIMIT,
                MotorCommand::MAX_VOLTAGE,
            );
        }

        state.cmd.voltage_limit = actual_limit;
    } else {
        warn_unplugged(&ctx, device, V5_DeviceType::kDeviceTypeMotorSensor);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn vexDeviceMotorVoltageLimitGet(device: V5_DeviceT) -> i32 {
    let mut ctx = DEVICES.lock();
    if let Some(state) = ctx.state_for::<MotorState>(device) {
        state.cmd.voltage_limit
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sdk::device::vexDeviceGetByIndex;

    use std::sync::atomic::{AtomicUsize, Ordering};

    use parking_lot::{Mutex, MutexGuard};

    use crate::device::{NUM_DEVICES_HANDLES, V5Device};

    /// These locks prevent multiple tests from using the same smart port when running in parallel.
    static PORT_LOCKS: [Mutex<()>; NUM_DEVICES_HANDLES] =
        [const { Mutex::new(()) }; NUM_DEVICES_HANDLES];

    /// A motor plugged into a smart port that this test has exclusive use of.
    struct TestMotor {
        port: usize,
        handle: V5_DeviceT,
        _port_lock: MutexGuard<'static, ()>,
    }

    impl TestMotor {
        fn state<R>(&self, f: impl FnOnce(&mut MotorState) -> R) -> R {
            f(DEVICES.lock().state_for::<MotorState>(self.port).unwrap())
        }

        /// The `pwm` value that will be sent to the physics simulator.
        fn commanded_pwm(&self) -> i32 {
            self.state(|s| s.cmd.pwm)
        }
    }

    /// Plug a new motor into a smart port and take exclusive use of it.
    ///
    /// If another test is running in parallel, the motor will be created on a different port to
    /// avoid conflicting with it. The device starts with no state.
    fn plug_in_motor() -> TestMotor {
        static NEXT_PORT: AtomicUsize = AtomicUsize::new(0);
        let port = NEXT_PORT.fetch_add(1, Ordering::Relaxed) % NUM_DEVICES_HANDLES;

        // Taking this before taking `devices` avoids a deadlock.
        let port_lock = PORT_LOCKS[port].lock();

        let mut devices = DEVICES.lock();
        let device = &mut devices.smart_devices[port];

        // Reset the device state since the last test.
        *device = V5Device::new();
        device.set_type(V5_DeviceType::kDeviceTypeMotorSensor);
        assert!(
            devices.state_for::<MotorState>(port).is_some(),
            "port {port} should hold motor state after plugging in a motor",
        );

        drop(devices);

        TestMotor {
            port,
            handle: vexDeviceGetByIndex(port as u32),
            _port_lock: port_lock,
        }
    }

    #[test]
    fn pwm_set_get_roundtrip() {
        let motor = plug_in_motor();
        for pwm in [-100, -37, 0, 42, 100] {
            vexDeviceMotorPwmSet(motor.handle, pwm);
            assert_eq!(vexDeviceMotorPwmGet(motor.handle), pwm);
            assert_eq!(motor.commanded_pwm(), pwm);
        }
    }

    #[test]
    fn pwm_set_clamps() {
        let motor = plug_in_motor();
        vexDeviceMotorPwmSet(motor.handle, 150);
        assert_eq!(motor.commanded_pwm(), 100);
        vexDeviceMotorPwmSet(motor.handle, -150);
        assert_eq!(motor.commanded_pwm(), -100);
    }

    #[test]
    fn pwm_get_undoes_reversal() {
        let motor = plug_in_motor();
        vexDeviceMotorReverseFlagSet(motor.handle, true);
        vexDeviceMotorPwmSet(motor.handle, 42);
        assert_eq!(motor.commanded_pwm(), -42);
        assert_eq!(vexDeviceMotorPwmGet(motor.handle), 42);
    }

    #[test]
    fn voltage_set_commands_percent() {
        let motor = plug_in_motor();
        for (voltage, pwm) in [
            (12_000, 100),
            (6_000, 50),
            (0, 0),
            (-3_000, -25),
            (-12_000, -100),
            (20_000, 100),
            (-20_000, -100),
        ] {
            vexDeviceMotorVoltageSet(motor.handle, voltage);
            assert_eq!(motor.commanded_pwm(), pwm, "voltage {voltage}");
            assert_eq!(vexDeviceMotorPwmGet(motor.handle), pwm, "voltage {voltage}");
        }
    }

    /// The voltage should be converted into a percent of the voltage limit, not of the max voltage.
    #[test]
    fn voltage_set_is_percent_of_limit() {
        let motor = plug_in_motor();
        vexDeviceMotorVoltageLimitSet(motor.handle, 6_000);
        for (voltage, pwm) in [
            (12_000, 100),
            (6_000, 100),
            (3_000, 50),
            (59, 0),
            (-1_500, -25),
            (-12_000, -100),
        ] {
            vexDeviceMotorVoltageSet(motor.handle, voltage);
            assert_eq!(motor.commanded_pwm(), pwm, "voltage {voltage}");
            assert_eq!(vexDeviceMotorPwmGet(motor.handle), pwm, "voltage {voltage}");
        }

        vexDeviceMotorReverseFlagSet(motor.handle, true);
        vexDeviceMotorVoltageSet(motor.handle, 3_000);
        assert_eq!(motor.commanded_pwm(), -50);
        assert_eq!(vexDeviceMotorPwmGet(motor.handle), 50);
    }

    #[test]
    fn voltage_limit_defaults_to_max() {
        let motor = plug_in_motor();
        assert_eq!(vexDeviceMotorVoltageLimitGet(motor.handle), 12_000);
    }

    /// The voltage limit is clamped to `[1000, 12000]`, and zero does not disable the motor or
    /// the voltage limit.
    #[test]
    fn voltage_limit_set_clamps() {
        let motor = plug_in_motor();
        for (limit, actual) in [
            (6_000, 6_000),
            (20_000, 12_000),
            (999, 1_000),
            (0, 1_000),
            (-5_000, 1_000),
        ] {
            vexDeviceMotorVoltageLimitSet(motor.handle, limit);
            assert_eq!(
                vexDeviceMotorVoltageLimitGet(motor.handle),
                actual,
                "limit {limit}"
            );
        }
    }
}
