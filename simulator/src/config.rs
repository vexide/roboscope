use std::{
    collections::HashSet,
    sync::{LazyLock, OnceLock},
};

use serde::Deserialize;

/// Get a reference to the active simulator config.
pub fn config() -> &'static Config {
    CONFIG.get().unwrap_or_else(|| &DEFAULT_CONFIG)
}

static DEFAULT_CONFIG: LazyLock<Config> = LazyLock::new(Config::default);
pub static CONFIG: OnceLock<Config> = OnceLock::new();

#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct Config {
    /// Flags for enabling debug behavior.
    pub debug: HashSet<DebugFlag>,
    /// Indicates whether the display's header canvas should be hidden from the display, expanding
    /// the user canvas mask to the full contents of the window.
    pub header_hidden: bool,
    /// The display theme to use (controls the erase color and default background color).
    pub theme: DisplayTheme,
    /// The battery capacity to show in the program header and report to user programs.
    pub battery_capacity: f64,
    /// Warning categories which should be suppressed.
    pub suppress_warnings: HashSet<Warning>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            debug: HashSet::new(),
            header_hidden: false,
            theme: DisplayTheme::default(),
            battery_capacity: 100.0,
            suppress_warnings: HashSet::new(),
        }
    }
}

/// Debug toggles for diagnosing misbehaving programs.
#[derive(Debug, Hash, PartialEq, Eq, Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DebugFlag {
    /// Writes the display's text buffer to "text_mask.png" after text-drawing operations.
    TextBuffer,
}

/// A category of warnings.
#[derive(Debug, Hash, PartialEq, Eq, Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Warning {
    /// An SDK function was called that isn't implemented.
    SdkUnimplemented,
    /// Tried to access a device that was missing (not plugged in/of the wrong type).
    MissingDevices,
    /// The given enum variant is unknown.
    UnknownEnumVariants,
}

/// The supported themes of the VEX V5's display.
#[derive(Debug, Hash, PartialEq, Eq, Clone, Copy, Default, Deserialize)]
pub enum DisplayTheme {
    #[default]
    Dark,
    Light,
}
