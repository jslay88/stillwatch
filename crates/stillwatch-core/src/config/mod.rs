//! The versioned Stillwatch config (`~/.config/stillwatch/config.toml`).
//!
//! Every section takes its documented defaults for missing keys and rejects
//! unknown ones. [`Config::from_toml_str`] runs the full pipeline: version
//! check, in-memory migration, deserialization, and [`Config::validate`].
//! Reading the file is left to the caller (`stillwatch_ipc::config_file`).

mod error;
pub mod limits;
mod load;
mod migrate;
mod sections;
mod validate;

use serde::{Deserialize, Serialize};

pub use error::ConfigError;
pub use limits::Bounds;
pub use load::LoadOutcome;
pub use migrate::{MIGRATIONS, MigrationNote, MigrationStep, migrate, rename_key};
pub use sections::{
    ActionConfig, ActionMode, ActionOutputs, ActivityConfig, BlankMethod, CaptureBackend,
    CaptureConfig, DimMethod, HistoryConfig, IdleConfig, IgnoreRegion, LogLevel, LoggingConfig,
    PanelCareConfig, PromptConfig, PromptStyle, PromptUrgency, ReblankFallback, SafetyConfig,
    SessionConfig, StaleConfig, StaleRequire, WhenLocked,
};
pub use validate::ValidationIssue;

/// The config schema version this build reads and writes.
pub const CURRENT_VERSION: u32 = 1;

/// The whole config file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Config schema version, used for automatic migrations.
    pub version: u32,
    /// `[idle]`
    pub idle: IdleConfig,
    /// `[session]`
    pub session: SessionConfig,
    /// `[activity]`
    pub activity: ActivityConfig,
    /// `[capture]`
    pub capture: CaptureConfig,
    /// `[stale]`
    pub stale: StaleConfig,
    /// `[safety]`
    pub safety: SafetyConfig,
    /// `[prompt]`
    pub prompt: PromptConfig,
    /// `[action]`
    pub action: ActionConfig,
    /// `[panel_care]`
    pub panel_care: PanelCareConfig,
    /// `[history]`
    pub history: HistoryConfig,
    /// `[logging]`
    pub logging: LoggingConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION,
            idle: IdleConfig::default(),
            session: SessionConfig::default(),
            activity: ActivityConfig::default(),
            capture: CaptureConfig::default(),
            stale: StaleConfig::default(),
            safety: SafetyConfig::default(),
            prompt: PromptConfig::default(),
            action: ActionConfig::default(),
            panel_care: PanelCareConfig::default(),
            history: HistoryConfig::default(),
            logging: LoggingConfig::default(),
        }
    }
}

impl Config {
    /// Serializes the config to TOML without comments.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Serialize`] if TOML serialization fails.
    pub fn to_toml_string(&self) -> Result<String, ConfigError> {
        Ok(toml::to_string(self)?)
    }
}

#[cfg(test)]
mod proptests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
