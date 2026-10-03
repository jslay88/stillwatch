//! In-memory upgrades of older config documents to [`CURRENT_VERSION`].
//!
//! Migrations run on the raw TOML table before deserialization, so a step can
//! rename or reshape keys that the current structs would reject. The daemon
//! logs the notes but never rewrites the file; the GUI writes the new version
//! when the user saves.

use std::fmt;

use toml::{Table, Value};

use super::{CURRENT_VERSION, ConfigError};

/// One upgrade step from version `from` to `from + 1`.
#[derive(Debug, Clone, Copy)]
pub struct MigrationStep {
    /// Version this step upgrades from.
    pub from: u32,
    /// Rewrites the table in place and returns a note for each change made.
    pub apply: fn(&mut Table) -> Vec<String>,
}

/// Every migration step this build carries, in any order.
///
/// Add a step here, and bump [`CURRENT_VERSION`], whenever a release renames,
/// moves, or reinterprets a key.
pub const MIGRATIONS: &[MigrationStep] = &[];

/// A change made while migrating, for logging or showing to the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationNote {
    /// Version the step upgraded from.
    pub from: u32,
    /// Version the step upgraded to.
    pub to: u32,
    /// What changed.
    pub message: String,
}

impl fmt::Display for MigrationNote {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{} -> v{}: {}", self.from, self.to, self.message)
    }
}

/// Upgrades `table` from version `from` to [`CURRENT_VERSION`] using [`MIGRATIONS`].
///
/// # Errors
///
/// [`ConfigError::VersionTooNew`] if `from` is newer than this build, or
/// [`ConfigError::VersionTooOld`] if a step in the chain is missing.
pub fn migrate(table: Table, from: u32) -> Result<(Table, Vec<MigrationNote>), ConfigError> {
    migrate_with(MIGRATIONS, table, from, CURRENT_VERSION)
}

pub(crate) fn migrate_with(
    steps: &[MigrationStep],
    mut table: Table,
    from: u32,
    to: u32,
) -> Result<(Table, Vec<MigrationNote>), ConfigError> {
    if from > to {
        return Err(ConfigError::VersionTooNew {
            found: from,
            supported: to,
        });
    }
    let mut notes = Vec::new();
    for version in from..to {
        let step =
            steps
                .iter()
                .find(|step| step.from == version)
                .ok_or(ConfigError::VersionTooOld {
                    found: from,
                    missing: version,
                })?;
        notes.extend(
            (step.apply)(&mut table)
                .into_iter()
                .map(|message| MigrationNote {
                    from: version,
                    to: version + 1,
                    message,
                }),
        );
    }
    table.insert("version".to_owned(), Value::Integer(i64::from(to)));
    Ok((table, notes))
}

/// Renames `[section] old` to `new`, for use inside a [`MigrationStep`].
///
/// Returns a note when the key was moved. Nothing happens if `old` is absent,
/// or if `new` is already set; in that case `old` stays put and is reported as
/// an unknown key when the config is parsed.
pub fn rename_key(table: &mut Table, section: &str, old: &str, new: &str) -> Option<String> {
    let section_table = table.get_mut(section)?.as_table_mut()?;
    if section_table.contains_key(new) {
        return None;
    }
    let value = section_table.remove(old)?;
    section_table.insert(new.to_owned(), value);
    Some(format!("renamed `{section}.{old}` to `{section}.{new}`"))
}

#[cfg(test)]
mod tests;
