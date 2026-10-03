//! `cargo xtask install` and `uninstall`.
//!
//! Builds the release binaries and copies them with the systemd user unit,
//! desktop files, and icons. The enable and disable `systemctl` lines are
//! printed. They are never run, so install cannot start `stillwatchd` or
//! blank a display.

mod plan;
mod prefix;
mod rewrite;

use std::env;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::process::Step;

pub use self::prefix::Prefix;

/// Builds the release binaries and installs them under `prefix`.
///
/// # Errors
///
/// Fails if the prefix is unusable, the build fails, or a file can't be written.
pub fn install(root: &Path, prefix: &str) -> Result<()> {
    let prefix = Prefix::parse(prefix, &home_dir()?)?;
    build_release(root)?;
    plan::require_binaries(root)?;
    let planned = plan::plan(root, &prefix)?;
    plan::apply(root, &planned, &prefix.bin_dir_string()?)?;
    eprintln!("{}", plan::enable_message(&prefix));
    Ok(())
}

/// Removes the files [`install`] would write for `prefix`.
///
/// # Errors
///
/// Fails if the prefix is unusable or a destination exists and can't be removed.
pub fn uninstall(root: &Path, prefix: &str) -> Result<()> {
    let prefix = Prefix::parse(prefix, &home_dir()?)?;
    let planned = plan::plan(root, &prefix)?;
    plan::remove(&planned)?;
    eprintln!("{}", plan::disable_message(&prefix));
    Ok(())
}

fn home_dir() -> Result<PathBuf> {
    env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .context("HOME is not set")
}

fn build_release(root: &Path) -> Result<()> {
    Step::new(
        "cargo",
        [
            "build",
            "--release",
            "--locked",
            "--bin",
            "stillwatchd",
            "--bin",
            "stillwatch",
            "--bin",
            "stillwatch-gui",
        ],
    )
    .run(root)
}
