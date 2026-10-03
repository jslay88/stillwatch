//! Parsing a config document: version check, migration, deserialization, validation.

use toml::{Table, Value};

use super::migrate::{MIGRATIONS, MigrationNote, MigrationStep, migrate_with};
use super::{CURRENT_VERSION, Config, ConfigError};

/// A successfully loaded and validated config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadOutcome {
    /// The validated config, at [`CURRENT_VERSION`].
    pub config: Config,
    /// The version the document was migrated from, if it was older.
    pub migrated_from: Option<u32>,
    /// Changes made by migration steps, in the order they ran.
    pub notes: Vec<MigrationNote>,
}

impl Config {
    /// Parses, migrates, and validates a TOML document.
    ///
    /// A missing `version` is treated as [`CURRENT_VERSION`], and missing
    /// sections or keys take their defaults.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Syntax`] for malformed TOML, a version error if
    /// the document is too new or too old, [`ConfigError::Parse`] for unknown
    /// keys or bad values, and [`ConfigError::Invalid`] with every broken rule.
    pub fn from_toml_str(input: &str) -> Result<LoadOutcome, ConfigError> {
        parse_with(MIGRATIONS, CURRENT_VERSION, input)
    }
}

pub(crate) fn parse_with(
    steps: &[MigrationStep],
    current: u32,
    input: &str,
) -> Result<LoadOutcome, ConfigError> {
    let table: Table = toml::from_str(input)?;
    let version = read_version(&table)?.unwrap_or(current);
    let (table, notes, migrated_from) = if version == current {
        (table, Vec::new(), None)
    } else {
        let (table, notes) = migrate_with(steps, table, version, current)?;
        (table, notes, Some(version))
    };
    let config: Config =
        serde_path_to_error::deserialize(table).map_err(|error| ConfigError::Parse {
            key: error.path().to_string(),
            message: error.into_inner().message().to_owned(),
        })?;
    config.validate().map_err(ConfigError::Invalid)?;
    Ok(LoadOutcome {
        config,
        migrated_from,
        notes,
    })
}

fn read_version(table: &Table) -> Result<Option<u32>, ConfigError> {
    let Some(value) = table.get("version") else {
        return Ok(None);
    };
    let invalid = || ConfigError::InvalidVersion {
        found: value.to_string(),
    };
    match value {
        Value::Integer(version) => u32::try_from(*version).map(Some).map_err(|_| invalid()),
        _ => Err(invalid()),
    }
}

#[cfg(test)]
mod tests;
