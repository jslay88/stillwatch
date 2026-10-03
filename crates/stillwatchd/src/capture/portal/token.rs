//! The portal restore token, so the monitor picker is shown once.
//!
//! xdg-desktop-portal returns a token from `Start` when
//! `persist_mode` is persistent. The next session sends it back and, if the
//! user hasn't revoked the grant, starts without a dialog. The file is
//! `$XDG_STATE_HOME/stillwatch/portal-restore-token`.

use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};

use stillwatch_core::backend::BackendError;
use stillwatch_ipc::paths::{self, PathsError};

/// File name inside the state directory.
pub const FILE_NAME: &str = "portal-restore-token";

/// Where the restore token is kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenStore {
    path: PathBuf,
}

impl TokenStore {
    /// A store at `path`.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// [`FILE_NAME`] inside `dir`.
    #[must_use]
    pub fn in_dir(dir: &Path) -> Self {
        Self::new(dir.join(FILE_NAME))
    }

    /// `$XDG_STATE_HOME/stillwatch/portal-restore-token`.
    ///
    /// # Errors
    ///
    /// Returns [`PathsError::NoStateDir`] if no state directory can be resolved.
    pub fn default_path() -> Result<PathBuf, PathsError> {
        Ok(paths::state_dir()?.join(FILE_NAME))
    }

    /// The file this store reads and writes.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The stored token, or `None` when the file is missing or empty.
    ///
    /// # Errors
    ///
    /// Returns [`BackendError::Io`] when the file exists but can't be read.
    pub fn load(&self) -> Result<Option<String>, BackendError> {
        match fs::read_to_string(&self.path) {
            Ok(text) => Ok(normalize(&text)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(BackendError::Io(format!(
                "can't read the portal restore token: {err}"
            ))),
        }
    }

    /// Replaces the stored token with the one from a `Start` response.
    ///
    /// An empty token leaves the file alone: the portal omits it when the
    /// previous grant still stands. A token is a single line; one with a
    /// newline is refused so the file stays one token.
    ///
    /// # Errors
    ///
    /// Returns [`BackendError::Protocol`] for a token that contains a newline,
    /// and [`BackendError::Io`] when the directory or file can't be written.
    pub fn store(&self, token: Option<&str>) -> Result<(), BackendError> {
        let Some(token) = token.and_then(normalize) else {
            return Ok(());
        };
        if token.contains(['\n', '\r']) {
            return Err(BackendError::Protocol(
                "portal restore token contains a newline".into(),
            ));
        }
        if let Some(parent) = self.path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&self.path)?;
        writeln!(file, "{token}")?;
        Ok(())
    }
}

fn normalize(text: &str) -> Option<String> {
    let token = text.trim();
    (!token.is_empty()).then(|| token.to_owned())
}

#[cfg(test)]
mod tests;
