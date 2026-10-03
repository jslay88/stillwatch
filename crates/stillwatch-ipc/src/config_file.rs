//! Reading and writing `config.toml` on disk.
//!
//! `stillwatch-core` only parses; this is where the file is actually read and
//! written, so the daemon, CLI, and GUI all map a missing file the same way and
//! all write it atomically.

use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

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

/// Why [`write`] failed.
#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    /// The file exists and overwriting wasn't allowed.
    #[error("{} already exists", path.display())]
    Exists {
        /// The existing file.
        path: PathBuf,
    },
    /// Creating the directory, writing the temp file, or renaming it failed.
    #[error("failed to write {}: {source}", path.display())]
    Io {
        /// The path being written.
        path: PathBuf,
        /// Underlying I/O error.
        source: io::Error,
    },
}

/// Writes `contents` to `path` atomically, creating missing parent directories.
///
/// The contents go to a temp file in the target directory, which is then
/// renamed over the target, so readers (and the daemon's watcher) never see a
/// half-written file. A symlinked config is followed and the file it points to
/// is replaced, so dotfile links survive. An existing file keeps its
/// permissions. With `overwrite` off, the rename refuses to replace an existing
/// file, even one created after the call started.
///
/// # Errors
///
/// Returns [`WriteError::Exists`] if the file exists and `overwrite` is off,
/// and [`WriteError::Io`] for any other failure.
pub fn write(path: &Path, contents: &str, overwrite: bool) -> Result<(), WriteError> {
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let io_error = |source| WriteError::Io {
        path: target.clone(),
        source,
    };
    let dir = match target.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        _ => Path::new("."),
    };
    std::fs::create_dir_all(dir).map_err(io_error)?;
    let mut temp = tempfile::Builder::new()
        .prefix(".stillwatch-")
        .suffix(".tmp")
        .tempfile_in(dir)
        .map_err(io_error)?;
    if let Ok(existing) = std::fs::metadata(&target) {
        temp.as_file()
            .set_permissions(existing.permissions())
            .map_err(io_error)?;
    }
    temp.write_all(contents.as_bytes()).map_err(io_error)?;
    temp.as_file().sync_all().map_err(io_error)?;
    let persisted = if overwrite {
        temp.persist(&target)
    } else {
        temp.persist_noclobber(&target)
    };
    persisted.map(drop).map_err(|err| match err.error.kind() {
        io::ErrorKind::AlreadyExists => WriteError::Exists {
            path: path.to_path_buf(),
        },
        _ => io_error(err.error),
    })
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
