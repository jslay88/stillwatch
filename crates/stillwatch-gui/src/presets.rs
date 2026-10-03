//! Snooze presets from the config file. The tray menu uses these without the
//! daemon, and again after `ConfigChanged`.

use stillwatch_core::config::{ConfigError, PromptConfig};
use stillwatch_ipc::config_file::{self, ConfigFileError};

/// `[prompt] snooze_presets_minutes`, or the defaults when the file is missing
/// or can't be read.
#[must_use]
pub fn load() -> Vec<u32> {
    let defaults = PromptConfig::default().snooze_presets_minutes;
    match config_file::load_default() {
        Ok(outcome) => outcome.config.prompt.snooze_presets_minutes,
        Err(ConfigFileError::Config(ConfigError::NotFound { .. })) => defaults,
        Err(err) => {
            tracing::warn!(%err, "using default snooze presets");
            defaults
        }
    }
}
