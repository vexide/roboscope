//! Commands sent from user code to devices.
//!
//! These events are intended to be consumed by physics simulators or device controllers.

use derive_more::{Deref, From, TryInto};
use iceoryx2::prelude::ZeroCopySend;
use vex_sdk::{V5MotorBrakeMode, V5MotorControlMode, V5MotorGearset};

use crate::SMART_DEVICES_COUNT;

/// A packet which reports the most recent commands from a robot controller to its peripherals.
#[derive(Debug, Copy, Clone, PartialEq, ZeroCopySend, Default, Deref)]
#[repr(C)]
pub struct RobotOutputs(pub [DeviceCommand; SMART_DEVICES_COUNT]);

/// A type-erased device command.
#[derive(Debug, Copy, Clone, PartialEq, ZeroCopySend, Default, From, TryInto)]
#[repr(C)]
pub enum DeviceCommand {
    #[default]
    Empty,
    Motor(MotorCommand),
}

#[derive(Debug, Copy, Clone, PartialEq, ZeroCopySend)]
#[repr(C)]
pub struct MotorCommand {
    /// Requested mode of motor control
    pub mode: MotorControlMode,
    /// Presumed gearset of motor
    pub gearset: MotorGearset,
    /// Target position in ticks
    pub target_position: i32,
    /// Target velocity in ticks per second
    pub target_velocity: i32,
    /// PWM control % with the range `[-100, 100]`.
    ///
    /// This is a percentage of [`Self::voltage_limit`], not of the motor's 12 V maximum, so the
    /// motor should output `pwm * voltage_limit / 100` millivolts.
    pub pwm: i32,
    /// Maximum output voltage in millivolts, with the range `[1000, 12000]`.
    pub voltage_limit: i32,
    /// Requested current limit in milliamperes.
    pub current_limit: i32,
    /// Brake mode for when targeting zero velocity.
    pub brake_mode: MotorBrakeMode,
}

impl MotorCommand {
    /// The highest voltage a motor can output, in millivolts.
    pub const MAX_VOLTAGE: i32 = 12_000;
    /// The lowest voltage limit a motor accepts, in millivolts.
    pub const MIN_VOLTAGE_LIMIT: i32 = 1_000;

    /// Returns the commanded PWM output in millivolts.
    ///
    /// This is intended be used only when the [mode](Self::mode) is [`MotorControlMode::Pwm`] to
    /// get the commanded output of the motor in millivolts.
    pub fn pwm_millivolts(&self) -> i32 {
        self.pwm * self.voltage_limit / 100
    }
}

impl Default for MotorCommand {
    fn default() -> Self {
        Self {
            mode: MotorControlMode::default(),
            gearset: MotorGearset::default(),
            target_position: 0,
            target_velocity: 0,
            pwm: 0,
            voltage_limit: Self::MAX_VOLTAGE,
            current_limit: 0,
            brake_mode: MotorBrakeMode::default(),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, ZeroCopySend, Default, From)]
#[repr(C)]
pub enum MotorBrakeMode {
    #[default]
    Coast,
    Brake,
    Hold,
}

impl From<MotorBrakeMode> for V5MotorBrakeMode {
    fn from(value: MotorBrakeMode) -> Self {
        V5MotorBrakeMode(value as u8)
    }
}

impl TryFrom<V5MotorBrakeMode> for MotorBrakeMode {
    type Error = ();
    fn try_from(value: V5MotorBrakeMode) -> Result<Self, Self::Error> {
        Ok(match value {
            V5MotorBrakeMode::kV5MotorBrakeModeCoast => Self::Coast,
            V5MotorBrakeMode::kV5MotorBrakeModeBrake => Self::Brake,
            V5MotorBrakeMode::kV5MotorBrakeModeHold => Self::Hold,
            _ => return Err(()),
        })
    }
}

#[derive(Debug, Copy, Clone, PartialEq, ZeroCopySend, Default, From)]
#[repr(C)]
pub enum MotorControlMode {
    /// The motor will be set to idle.
    #[default]
    Off,
    /// The motor will apply a braking force.
    Brake,
    /// The motor will use position control to hold its current rotation.
    Hold,
    /// The motor will use position control to rotate to the target position.
    Position = 4,
    /// The motor will use velocity control to rotate at the target velocity.
    Velocity,
    /// The motor will use pulse-width modulation to rotate using a percent of the max voltage.
    Pwm,
}

impl TryFrom<V5MotorControlMode> for MotorControlMode {
    type Error = ();

    fn try_from(value: V5MotorControlMode) -> Result<Self, Self::Error> {
        Ok(match value {
            V5MotorControlMode::kMotorControlModeOFF => Self::Off,
            V5MotorControlMode::kMotorControlModeBRAKE => Self::Brake,
            V5MotorControlMode::kMotorControlModeHOLD => Self::Hold,
            V5MotorControlMode::kMotorControlModePROFILE => Self::Position,
            V5MotorControlMode::kMotorControlModeVELOCITY => Self::Velocity,
            _ => return Err(()),
        })
    }
}

impl TryFrom<MotorControlMode> for V5MotorControlMode {
    type Error = ();

    fn try_from(value: MotorControlMode) -> Result<Self, Self::Error> {
        if value == MotorControlMode::Pwm {
            return Err(());
        }

        Ok(V5MotorControlMode(value as u8))
    }
}

/// A known gear ratio cartridge.
#[derive(Debug, Copy, Clone, PartialEq, ZeroCopySend, Default, From)]
#[repr(C)]
pub enum MotorGearset {
    /// 36&times; slower than the base motor speed. (Red cartridge)
    Ratio36To1,
    /// The V5's "Standard" gearing, 18&times; slower than the base motor speed. (Green cartridge)
    #[default]
    Ratio18To1,
    /// 6&times; slower than the base motor speed. (Blue cartridge)
    Ratio06To1,
}

impl MotorGearset {
    /// Try to convert a VEX SDK gearset to a known gearset.
    ///
    /// Returns `None` if the gearset kind is unknown.
    pub const fn new(gearset: V5MotorGearset) -> Option<Self> {
        Some(match gearset {
            V5MotorGearset::kMotorGearSet_06 => Self::Ratio06To1,
            V5MotorGearset::kMotorGearSet_18 => Self::Ratio18To1,
            V5MotorGearset::kMotorGearSet_36 => Self::Ratio36To1,
            _ => return None,
        })
    }

    #[must_use]
    pub const fn to_v5(self) -> V5MotorGearset {
        V5MotorGearset(self as u8)
    }

    /// Get the conversion ratio for rotation through this gearset.
    #[must_use]
    pub const fn multiplier(self) -> f64 {
        match self {
            Self::Ratio36To1 => 1.0 / 36.0,
            Self::Ratio18To1 => 1.0 / 18.0,
            Self::Ratio06To1 => 1.0 / 6.0,
        }
    }
}
