//! Errors from loading, migrating, validating, and writing config.

use std::fmt::Write as _;
use std::io;
use std::path::PathBuf;

use super::ValidationIssue;

/// Why a config could not be loaded or written.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// The config file doesn't exist; callers like the daemon run on defaults.
    ///
    /// Core never reads files; the file loader in `stillwatch-ipc` produces
    /// this and [`ConfigError::Io`].
    #[error("config file not found: {}", path.display())]
    NotFound {
        /// Path that was looked up.
        path: PathBuf,
    },
    /// The config file exists but couldn't be read.
    #[error("failed to read {}: {source}", path.display())]
    Io {
        /// Path that failed to read.
        path: PathBuf,
        /// Underlying I/O error.
        source: io::Error,
    },
    /// The file isn't valid TOML.
    #[error(transparent)]
    Syntax(#[from] toml::de::Error),
    /// `version` is present but isn't a non-negative integer.
    #[error("invalid `version`: expected a non-negative integer, got {found}")]
    InvalidVersion {
        /// The offending value as written in the file.
        found: String,
    },
    /// The file was written by a newer Stillwatch.
    #[error(
        "config version {found} is newer than this build supports (version {supported}); \
         upgrade Stillwatch or lower `version`"
    )]
    VersionTooNew {
        /// Version found in the file.
        found: u32,
        /// Newest version this build understands.
        supported: u32,
    },
    /// The file is older than any migration this build carries.
    #[error(
        "config version {found} is too old for this build: no migration from version {missing}"
    )]
    VersionTooOld {
        /// Version found in the file.
        found: u32,
        /// First version in the chain without a migration step.
        missing: u32,
    },
    /// A key is unknown or a value has the wrong type or an unknown enum variant.
    #[error("invalid config at `{key}`: {message}")]
    Parse {
        /// Dotted key path, such as `stale.require` or `stale.ignore_regions[0].w`.
        key: String,
        /// What serde rejected.
        message: String,
    },
    /// The config parsed but broke one or more validation rules.
    #[error("invalid config:{}", issue_list(.0))]
    Invalid(Vec<ValidationIssue>),
    /// The config couldn't be serialized to TOML.
    #[error("failed to serialize config: {0}")]
    Serialize(#[from] toml::ser::Error),
}

fn issue_list(issues: &[ValidationIssue]) -> String {
    issues.iter().fold(String::new(), |mut list, issue| {
        let _ = write!(list, "\n  {issue}");
        list
    })
}
