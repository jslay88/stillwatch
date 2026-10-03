//! `[panel_care]`, `[history]`, and `[logging]`.

use serde::{Deserialize, Serialize};

use crate::config::validate::Issues;

/// `[panel_care]`: screen-on tracking so the panel's compensation cycle can run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PanelCareConfig {
    /// Whether screen-on time is tracked.
    pub enabled: bool,
    /// Minutes in standby that reset screen-on time.
    pub min_standby_minutes: u32,
    /// Whether to remind the user once screen-on time passes `reminder_hours`.
    pub reminder_enabled: bool,
    /// Screen-on hours before the reminder.
    pub reminder_hours: u32,
    /// Command run at blank time when panel care is due.
    pub trigger_cmd: String,
}

impl Default for PanelCareConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_standby_minutes: 10,
            reminder_enabled: true,
            reminder_hours: 4,
            trigger_cmd: String::new(),
        }
    }
}

impl PanelCareConfig {
    pub(crate) fn validate(&self, issues: &mut Issues) {
        issues.at_least("panel_care.reminder_hours", self.reminder_hours, 1);
    }
}

/// `[history]`: the decision history ring.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HistoryConfig {
    /// Whether decisions are recorded.
    pub enabled: bool,
    /// Entries kept in the ring file.
    pub max_entries: u32,
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_entries: 1000,
        }
    }
}

impl HistoryConfig {
    pub(crate) fn validate(&self, issues: &mut Issues) {
        issues.at_least("history.max_entries", self.max_entries, 1);
    }
}

/// Log verbosity.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    /// Errors only.
    Error,
    /// Warnings and errors.
    Warn,
    /// Informational messages and above.
    #[default]
    Info,
    /// Debug messages and above.
    Debug,
    /// Everything.
    Trace,
}

/// `[logging]`: log verbosity.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LoggingConfig {
    /// Log level.
    pub level: LogLevel,
}
