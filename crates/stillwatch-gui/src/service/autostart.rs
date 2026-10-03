//! Tray autostart desktop file under `$XDG_CONFIG_HOME/autostart`.
//!
//! `cargo xtask install` puts the template in `share/stillwatch` and does not
//! copy it here. The Service page does.

use std::path::{Path, PathBuf};

/// File name in the autostart directory.
pub const FILE_NAME: &str = "io.github.jslay88.Stillwatch.Tray.desktop";

const TEMPLATE: &str =
    include_str!("../../../../packaging/io.github.jslay88.Stillwatch.Tray.desktop");

/// The desktop file the toggle writes.
#[must_use]
pub fn template() -> &'static str {
    TEMPLATE
}

/// `config_home/autostart/`[`FILE_NAME`].
#[must_use]
pub fn desktop_path(config_home: &Path) -> PathBuf {
    config_home.join("autostart").join(FILE_NAME)
}

/// The file is present.
#[must_use]
pub fn is_enabled(config_home: &Path) -> bool {
    desktop_path(config_home).is_file()
}

/// Writes the template, or removes the file.
///
/// # Errors
///
/// Returns the I/O error from creating the directory, writing, or removing.
pub fn set_enabled(config_home: &Path, enabled: bool) -> std::io::Result<()> {
    let path = desktop_path(config_home);
    if !enabled {
        return remove_file(&path);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, template())
}

/// `$XDG_CONFIG_HOME`, or `$HOME/.config` when that variable is unset or empty.
#[must_use]
pub fn config_home_from(xdg: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    if let Some(xdg) = xdg.filter(|path| !path.as_os_str().is_empty()) {
        return Some(xdg.to_path_buf());
    }
    home.map(|home| home.join(".config"))
}

pub(crate) fn user_config_home() -> Option<PathBuf> {
    let xdg = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
    let home = std::env::var_os("HOME").map(PathBuf::from);
    config_home_from(xdg.as_deref(), home.as_deref())
}

fn remove_file(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}
