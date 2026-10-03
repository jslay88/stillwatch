//! Validation that reports every problem at once, each keyed by its dotted path.

use std::fmt;
use std::ops::RangeInclusive;

use super::{CURRENT_VERSION, Config};

/// One validation problem, keyed by the dotted path of the offending setting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationIssue {
    /// Dotted key path, such as `stale.stale_percent` or `stale.ignore_regions[0].w`.
    pub key: String,
    /// Human-readable description of what is wrong.
    pub message: String,
}

impl fmt::Display for ValidationIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.key, self.message)
    }
}

/// Collects issues while the sections check themselves.
#[derive(Debug, Default)]
pub(crate) struct Issues(Vec<ValidationIssue>);

impl Issues {
    pub(crate) fn push(&mut self, key: impl Into<String>, message: impl Into<String>) {
        self.0.push(ValidationIssue {
            key: key.into(),
            message: message.into(),
        });
    }

    pub(crate) fn range(&mut self, key: &str, value: u32, range: RangeInclusive<u32>) {
        if !range.contains(&value) {
            self.push(
                key,
                format!(
                    "must be between {} and {}, got {value}",
                    range.start(),
                    range.end()
                ),
            );
        }
    }

    pub(crate) fn at_least(&mut self, key: &str, value: u32, min: u32) {
        if value < min {
            self.push(key, format!("must be at least {min}, got {value}"));
        }
    }

    pub(crate) fn not_blank(&mut self, key: &str, value: &str) {
        if value.trim().is_empty() {
            self.push(key, "must not be empty");
        }
    }

    pub(crate) fn entries_not_blank(&mut self, key: &str, values: &[String]) {
        for (index, value) in values.iter().enumerate() {
            self.not_blank(&format!("{key}[{index}]"), value);
        }
    }

    fn into_result(self) -> Result<(), Vec<ValidationIssue>> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(self.0)
        }
    }
}

impl Config {
    /// Checks every rule and returns all problems found, not just the first.
    ///
    /// # Errors
    ///
    /// Returns every [`ValidationIssue`] when at least one rule fails.
    pub fn validate(&self) -> Result<(), Vec<ValidationIssue>> {
        let mut issues = Issues::default();
        if self.version != CURRENT_VERSION {
            issues.push(
                "version",
                format!("must be {CURRENT_VERSION}, got {}", self.version),
            );
        }
        self.idle.validate(&mut issues);
        self.activity.validate(&mut issues);
        self.stale.validate(&mut issues);
        self.safety.validate(&self.stale, &mut issues);
        self.prompt.validate(&mut issues);
        self.action.validate(&mut issues);
        self.panel_care.validate(&mut issues);
        self.history.validate(&mut issues);
        issues.into_result()
    }
}

#[cfg(test)]
mod tests;
