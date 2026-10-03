//! Reading `config.toml` from disk.
//!
//! `stillwatch-core` only parses; this is where the file is actually read, so
//! the daemon, CLI, and GUI all map a missing file the same way.

use std::io;
use std::path::Path;

use stillwatch_core::config::{Config, ConfigError, LoadOutcome};

use crate::paths::{self, PathsError};

/// Why [`load_default`] failed.
#[derive(Debug, thiserror::Error)]
pub enum ConfigFileError {
    /// The standard config path couldn't be resolved.
    #[error(transparent)]
    Paths(#[from] PathsError),
    /// The file couldn't be read, parsed, migrated, or validated.
    #[error(transparent)]
    Config(#[from] ConfigError),
}

/// Reads and parses the config file at `path`.
///
/// # Errors
///
/// Returns [`ConfigError::NotFound`] if the file doesn't exist, so the caller
/// can fall back to defaults, [`ConfigError::Io`] for other read failures, and
/// otherwise anything [`Config::from_toml_str`] returns.
pub fn load(path: &Path) -> Result<LoadOutcome, ConfigError> {
    let input = std::fs::read_to_string(path).map_err(|source| {
        let path = path.to_path_buf();
        if source.kind() == io::ErrorKind::NotFound {
            ConfigError::NotFound { path }
        } else {
            ConfigError::Io { path, source }
        }
    })?;
    Config::from_toml_str(&input)
}

/// Reads and parses `$XDG_CONFIG_HOME/stillwatch/config.toml`.
///
/// # Errors
///
/// Returns [`ConfigFileError::Paths`] if the config directory can't be
/// resolved, and otherwise whatever [`load`] returns.
pub fn load_default() -> Result<LoadOutcome, ConfigFileError> {
    Ok(load(&paths::config_file()?)?)
}

#[cfg(test)]
mod tests;
