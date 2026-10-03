//! Standard file locations shared by the daemon, CLI, and GUI.
//!
//! The `*_in` functions take the base directory explicitly so they can be
//! tested without touching the environment. The wrappers resolve the base from
//! the XDG variables (with the usual `$HOME` fallbacks) through `dirs`.

use std::path::{Path, PathBuf};

/// Directory name used under the XDG config and state directories.
pub const APP_DIR: &str = "stillwatch";

/// File name of the config file inside the config directory.
pub const CONFIG_FILE: &str = "config.toml";

/// Errors from resolving the standard directories.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PathsError {
    /// Neither `$XDG_CONFIG_HOME` nor `$HOME` could be resolved.
    #[error("can't find the config directory (is $HOME set?)")]
    NoConfigDir,
    /// Neither `$XDG_STATE_HOME` nor `$HOME` could be resolved.
    #[error("can't find the state directory (is $HOME set?)")]
    NoStateDir,
}

/// Stillwatch's config directory under the given XDG config base.
#[must_use]
pub fn config_dir_in(config_base: &Path) -> PathBuf {
    config_base.join(APP_DIR)
}

/// Path of `config.toml` under the given XDG config base.
#[must_use]
pub fn config_file_in(config_base: &Path) -> PathBuf {
    config_dir_in(config_base).join(CONFIG_FILE)
}

/// Stillwatch's state directory under the given XDG state base.
#[must_use]
pub fn state_dir_in(state_base: &Path) -> PathBuf {
    state_base.join(APP_DIR)
}

/// `$XDG_CONFIG_HOME/stillwatch`, normally `~/.config/stillwatch`.
///
/// # Errors
///
/// Returns [`PathsError::NoConfigDir`] if no config base can be resolved.
pub fn config_dir() -> Result<PathBuf, PathsError> {
    resolve(dirs::config_dir(), PathsError::NoConfigDir, config_dir_in)
}

/// `$XDG_CONFIG_HOME/stillwatch/config.toml`, normally
/// `~/.config/stillwatch/config.toml`.
///
/// # Errors
///
/// Returns [`PathsError::NoConfigDir`] if no config base can be resolved.
pub fn config_file() -> Result<PathBuf, PathsError> {
    resolve(dirs::config_dir(), PathsError::NoConfigDir, config_file_in)
}

/// `$XDG_STATE_HOME/stillwatch`, normally `~/.local/state/stillwatch`.
///
/// # Errors
///
/// Returns [`PathsError::NoStateDir`] if no state base can be resolved.
pub fn state_dir() -> Result<PathBuf, PathsError> {
    resolve(dirs::state_dir(), PathsError::NoStateDir, state_dir_in)
}

fn resolve(
    base: Option<PathBuf>,
    missing: PathsError,
    build: fn(&Path) -> PathBuf,
) -> Result<PathBuf, PathsError> {
    base.map(|base| build(&base)).ok_or(missing)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_paths_join_app_dir_and_file() {
        let base = Path::new("/tmp/xdg-config");
        assert_eq!(config_dir_in(base), Path::new("/tmp/xdg-config/stillwatch"));
        assert_eq!(
            config_file_in(base),
            Path::new("/tmp/xdg-config/stillwatch/config.toml")
        );
    }

    #[test]
    fn state_dir_joins_app_dir() {
        let base = Path::new("/home/me/.local/state");
        assert_eq!(
            state_dir_in(base),
            Path::new("/home/me/.local/state/stillwatch")
        );
    }

    #[test]
    fn resolve_reports_missing_base() {
        assert_eq!(
            resolve(None, PathsError::NoStateDir, state_dir_in),
            Err(PathsError::NoStateDir)
        );
    }

    #[test]
    fn wrappers_match_dirs_bases() {
        assert_eq!(
            config_dir().ok(),
            dirs::config_dir().map(|base| config_dir_in(&base))
        );
        assert_eq!(
            config_file().ok(),
            dirs::config_dir().map(|base| config_file_in(&base))
        );
        assert_eq!(
            state_dir().ok(),
            dirs::state_dir().map(|base| state_dir_in(&base))
        );
    }

    #[test]
    fn errors_mention_home() {
        assert!(PathsError::NoConfigDir.to_string().contains("$HOME"));
        assert!(PathsError::NoStateDir.to_string().contains("$HOME"));
    }
}
