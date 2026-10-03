//! `stillwatch config` commands, which work on the file directly.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context as _, anyhow, bail};
use stillwatch_core::config::{CURRENT_VERSION, ConfigError, LoadOutcome};
use stillwatch_core::schema;
use stillwatch_ipc::config_file::{self, WriteError};
use stillwatch_ipc::paths;

use crate::args::{ConfigCheckArgs, ConfigInitArgs};

/// `stillwatch config init`: writes the commented default config.
///
/// # Errors
///
/// Fails if the file exists and `--force` wasn't given, or if it can't be written.
pub fn init(args: &ConfigInitArgs) -> anyhow::Result<()> {
    let path = path_or_default(args.path.as_deref())?;
    init_at(&path, args.force, &mut io::stdout().lock())
}

/// `stillwatch config check`: loads, migrates in memory, and validates a file.
///
/// Every problem is printed to stdout as `key: message`.
///
/// # Errors
///
/// Fails if the file is missing, unreadable, or invalid.
pub fn check(args: &ConfigCheckArgs) -> anyhow::Result<()> {
    let path = path_or_default(args.path.as_deref())?;
    check_at(&path, &mut io::stdout().lock())
}

fn path_or_default(path: Option<&Path>) -> anyhow::Result<PathBuf> {
    match path {
        Some(path) => Ok(path.to_path_buf()),
        None => Ok(paths::config_file()?),
    }
}

fn init_at(path: &Path, force: bool, out: &mut dyn Write) -> anyhow::Result<()> {
    let contents = schema::commented_toml()?;
    config_file::write(path, &contents, force).map_err(|err| match err {
        WriteError::Exists { .. } => anyhow!("{err}; pass --force to overwrite it"),
        WriteError::Io { .. } => err.into(),
    })?;
    writeln!(out, "wrote {}", path.display())?;
    Ok(())
}

fn check_at(path: &Path, out: &mut dyn Write) -> anyhow::Result<()> {
    let shown = path.display();
    let error = match config_file::load(path) {
        Ok(outcome) => return report_valid(path, &outcome, out),
        Err(ConfigError::NotFound { .. }) => {
            bail!("{shown} doesn't exist; `stillwatch config init` creates it")
        }
        Err(error) => error,
    };
    let issues = error.keyed_issues();
    if issues.is_empty() {
        return Err(error).with_context(|| format!("{shown} is invalid"));
    }
    for issue in &issues {
        writeln!(out, "{issue}")?;
    }
    let count = issues.len();
    let noun = if count == 1 { "problem" } else { "problems" };
    bail!("{shown} is invalid: {count} {noun}")
}

fn report_valid(path: &Path, outcome: &LoadOutcome, out: &mut dyn Write) -> anyhow::Result<()> {
    if let Some(from) = outcome.migrated_from {
        writeln!(
            out,
            "migrated from version {from} to {CURRENT_VERSION} in memory; the file is unchanged"
        )?;
        for note in &outcome.notes {
            writeln!(out, "  {note}")?;
        }
    }
    writeln!(out, "{}: ok", path.display())?;
    Ok(())
}

#[cfg(test)]
mod tests;
