//! `[idle]`, `[session]`, and `[activity]`: when the user counts as away.

use serde::{Deserialize, Serialize};

use crate::config::limits::{PERCENT, POSITIVE};
use crate::config::validate::Issues;

/// `[idle]`: how long input must be idle before monitoring starts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct IdleConfig {
    /// Minutes without keyboard, mouse, or gamepad input before the user counts as idle.
    pub input_idle_minutes: u32,
}

impl Default for IdleConfig {
    fn default() -> Self {
        Self {
            input_idle_minutes: 10,
        }
    }
}

impl IdleConfig {
    pub(crate) fn validate(&self, issues: &mut Issues) {
        issues.range("idle.input_idle_minutes", self.input_idle_minutes, POSITIVE);
    }
}

/// What to do while the session is locked.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WhenLocked {
    /// Wait for unlock without blanking.
    Pause,
    /// Blank once the session has been locked for `locked_blank_seconds`.
    #[default]
    BlankAfter,
}

/// `[session]`: behavior while the session is locked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SessionConfig {
    /// Locked-session behavior.
    pub when_locked: WhenLocked,
    /// Seconds locked before blanking when `when_locked = "blank_after"`.
    pub locked_blank_seconds: u32,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            when_locked: WhenLocked::default(),
            locked_blank_seconds: 60,
        }
    }
}

/// `[activity]`: gamepad input as an activity source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ActivityConfig {
    /// Whether gamepad events count as input.
    pub gamepad: bool,
    /// Stick movement below this percentage of full travel is ignored (stick drift).
    pub gamepad_deadzone_percent: u32,
    /// Device name substrings to ignore, such as a drifting pad or a sim rig.
    pub gamepad_ignore_devices: Vec<String>,
    /// Whether gamepad input wakes blanked displays.
    pub gamepad_wakes_display: bool,
}

impl Default for ActivityConfig {
    fn default() -> Self {
        Self {
            gamepad: true,
            gamepad_deadzone_percent: 15,
            gamepad_ignore_devices: Vec::new(),
            gamepad_wakes_display: true,
        }
    }
}

impl ActivityConfig {
    /// Whether `name` contains any [`Self::gamepad_ignore_devices`] entry.
    ///
    /// Comparison is case-insensitive. Blank entries never match, so a drifting
    /// pad is ignored only when its reported name actually contains a needle.
    #[must_use]
    pub fn ignores_device(&self, name: &str) -> bool {
        ignores_device_name(&self.gamepad_ignore_devices, name)
    }

    pub(crate) fn validate(&self, issues: &mut Issues) {
        issues.range(
            "activity.gamepad_deadzone_percent",
            self.gamepad_deadzone_percent,
            PERCENT,
        );
        issues.entries_not_blank(
            "activity.gamepad_ignore_devices",
            &self.gamepad_ignore_devices,
        );
    }
}

/// Whether `name` contains any ignore entry, compared case-insensitively.
///
/// Blank entries never match. The daemon's hot path and the settings picker
/// both use this so a selected gamepad is the same substring the source ignores.
#[must_use]
pub fn ignores_device_name(needles: &[impl AsRef<str>], name: &str) -> bool {
    let folded = name.to_lowercase();
    needles.iter().any(|needle| {
        let needle = needle.as_ref().trim().to_lowercase();
        !needle.is_empty() && folded.contains(&needle)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignore_entries_match_case_insensitive_substrings() {
        let config = ActivityConfig {
            gamepad_ignore_devices: vec!["Drifty".into(), " ".into()],
            ..ActivityConfig::default()
        };
        assert!(config.ignores_device("8BitDo Drifty Pad"));
        assert!(config.ignores_device("drifty pro"));
        assert!(!config.ignores_device("stable pad"));
        assert!(!ignores_device_name(&[" ".to_owned()], "stable pad"));
    }
}
