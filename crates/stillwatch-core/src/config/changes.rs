//! What a reload changed, setting by setting.
//!
//! The comparison walks the settings schema, so a new setting is covered as
//! soon as it has a schema entry, and which keys reset detection comes from
//! [`Setting::resets_detection`] rather than a second list.

use super::Config;
use crate::schema::{self, Setting};

/// The settings that differ between two configs, in schema order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigChanges {
    settings: Vec<&'static Setting>,
}

impl ConfigChanges {
    /// Compares `old` and `new` key by key.
    ///
    /// Values compare as serialized, so reordering a list counts as a change.
    /// If either config can't be serialized, every setting counts as changed,
    /// which makes the daemon rebuild everything rather than miss a change.
    #[must_use]
    pub fn between(old: &Config, new: &Config) -> Self {
        let (Ok(old), Ok(new)) = (schema::table_of(old), schema::table_of(new)) else {
            return Self {
                settings: schema::settings().collect(),
            };
        };
        let settings = schema::settings()
            .filter(|setting| {
                schema::lookup(&old, setting.key) != schema::lookup(&new, setting.key)
            })
            .collect();
        Self { settings }
    }

    /// Whether nothing changed.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.settings.is_empty()
    }

    /// The changed settings.
    pub fn settings(&self) -> impl Iterator<Item = &'static Setting> + '_ {
        self.settings.iter().copied()
    }

    /// The changed dotted keys, such as `stale.stale_percent`.
    pub fn keys(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.settings().map(|setting| setting.key)
    }

    /// Whether `key` changed.
    #[must_use]
    pub fn contains(&self, key: &str) -> bool {
        self.keys().any(|changed| changed == key)
    }

    /// Whether any key in the section with table name `id` (such as
    /// `history`) changed. Top-level keys like `version` have the empty id.
    #[must_use]
    pub fn section_changed(&self, id: &str) -> bool {
        self.keys()
            .any(|key| key.rsplit_once('.').map_or("", |(section, _)| section) == id)
    }

    /// Whether the capture backend has to be rebuilt and the detector's
    /// block counters started over: `capture.backend`, `stale.block_grid`,
    /// or `stale.monitored_outputs` changed.
    #[must_use]
    pub fn resets_detection(&self) -> bool {
        self.settings().any(|setting| setting.resets_detection)
    }

    /// Whether the `[history]` section changed, so the history ring has to
    /// pick up the new size or switch recording on or off.
    #[must_use]
    pub fn history_changed(&self) -> bool {
        self.section_changed("history")
    }
}

#[cfg(test)]
mod tests;
