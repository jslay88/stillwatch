//! The settings schema: one entry per config key, the single source of truth
//! for the settings window, `stillwatch config init`, and `docs/config.md`.
//!
//! Defaults aren't repeated here. They come from serializing
//! [`Config::default`] (see [`default_table`]), and numeric ranges are the same
//! [`crate::config::limits`] constants validation uses.

mod action;
mod activity;
mod capture;
mod general;
mod history;
mod idle;
mod logging;
mod markdown;
mod panel_care;
mod prompt;
mod safety;
mod session;
mod stale;
mod toml_gen;
mod types;

use toml::{Table, Value};

use crate::config::{Config, ConfigError};

pub use markdown::markdown_reference;
pub use toml_gen::commented_toml;
pub use types::{Choice, Control, Section, Setting, TimeUnit};

/// Every section, in config file order. Top-level keys come first.
pub const SECTIONS: &[Section] = &[
    general::SECTION,
    idle::SECTION,
    session::SECTION,
    activity::SECTION,
    capture::SECTION,
    stale::SECTION,
    safety::SECTION,
    prompt::SECTION,
    action::SECTION,
    panel_care::SECTION,
    history::SECTION,
    logging::SECTION,
];

/// Every setting, in config file order.
pub fn settings() -> impl Iterator<Item = &'static Setting> {
    SECTIONS.iter().flat_map(|section| section.settings)
}

/// The setting for a key path.
///
/// Indexed paths from validation issues, such as `stale.ignore_regions[0].w`,
/// map to their list setting.
#[must_use]
pub fn find(key: &str) -> Option<&'static Setting> {
    let key = key.split_once('[').map_or(key, |(list, _)| list);
    settings().find(|setting| setting.key == key)
}

/// [`Config::default`] as a TOML table, for looking up defaults with
/// [`Setting::default_in`] or [`lookup`].
///
/// # Errors
///
/// Returns [`ConfigError::Serialize`] if the config can't be serialized.
pub fn default_table() -> Result<Table, ConfigError> {
    table_of(&Config::default())
}

/// A config as a TOML table, keyed the same way as the schema.
///
/// # Errors
///
/// Returns [`ConfigError::Serialize`] if the config can't be serialized.
pub fn table_of(config: &Config) -> Result<Table, ConfigError> {
    Ok(Table::try_from(config)?)
}

/// The value at a dotted key path such as `stale.block_grid`.
#[must_use]
pub fn lookup<'a>(table: &'a Table, key: &str) -> Option<&'a Value> {
    let mut parts = key.split('.');
    let mut value = table.get(parts.next()?)?;
    for part in parts {
        value = value.as_table()?.get(part)?;
    }
    Some(value)
}

/// Every leaf key path in `table`, such as `stale.stale_percent`. Arrays,
/// including arrays of tables such as `stale.ignore_regions`, count as one
/// leaf, matching how the schema keys them.
#[must_use]
pub fn leaf_keys(table: &Table) -> Vec<String> {
    let mut keys = Vec::new();
    collect_leaves(table, "", &mut keys);
    keys
}

fn collect_leaves(table: &Table, prefix: &str, keys: &mut Vec<String>) {
    for (name, value) in table {
        let key = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}.{name}")
        };
        match value {
            Value::Table(inner) => collect_leaves(inner, &key, keys),
            _ => keys.push(key),
        }
    }
}

#[cfg(test)]
mod tests;
