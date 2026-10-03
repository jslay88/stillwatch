//! `[prompt]`: the snooze prompt shown before acting.

use serde::{Deserialize, Serialize};

use crate::config::validate::Issues;

/// How the prompt is shown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptStyle {
    /// Notification normally, dialog when a notification wouldn't be seen.
    #[default]
    Auto,
    /// Desktop notification with actions.
    Notification,
    /// Stillwatch's own dialog window.
    Dialog,
}

/// Notification urgency for the prompt.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptUrgency {
    /// Low urgency.
    Low,
    /// Normal urgency.
    Normal,
    /// Critical urgency, shown even in Do Not Disturb.
    #[default]
    Critical,
}

/// `[prompt]`: countdown and snooze choices.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PromptConfig {
    /// How the prompt is shown.
    pub style: PromptStyle,
    /// Notification urgency.
    pub urgency: PromptUrgency,
    /// Show the dialog when the notification fails or is dismissed.
    pub fallback_to_dialog: bool,
    /// Seconds before the action runs if nobody answers.
    pub countdown_seconds: u32,
    /// Snooze durations offered as buttons, in minutes.
    pub snooze_presets_minutes: Vec<u32>,
    /// Whether a custom snooze duration can be entered.
    pub allow_custom: bool,
    /// Shortest snooze, in minutes.
    pub custom_min_minutes: u32,
    /// Longest snooze, in minutes.
    pub custom_max_minutes: u32,
    /// Whether input during a snooze cancels it.
    pub snooze_cancelled_by_input: bool,
}

impl Default for PromptConfig {
    fn default() -> Self {
        Self {
            style: PromptStyle::default(),
            urgency: PromptUrgency::default(),
            fallback_to_dialog: true,
            countdown_seconds: 60,
            snooze_presets_minutes: vec![15, 60, 180],
            allow_custom: true,
            custom_min_minutes: 1,
            custom_max_minutes: 720,
            snooze_cancelled_by_input: false,
        }
    }
}

impl PromptConfig {
    pub(crate) fn validate(&self, issues: &mut Issues) {
        issues.at_least("prompt.countdown_seconds", self.countdown_seconds, 1);
        issues.at_least("prompt.custom_min_minutes", self.custom_min_minutes, 1);
        if self.snooze_presets_minutes.is_empty() && !self.allow_custom {
            issues.push(
                "prompt.snooze_presets_minutes",
                "must not be empty when allow_custom is false",
            );
        }
        let (min, max) = (self.custom_min_minutes, self.custom_max_minutes);
        if min > max {
            issues.push(
                "prompt.custom_max_minutes",
                format!("must be at least custom_min_minutes ({min}), got {max}"),
            );
            return;
        }
        for (index, preset) in self.snooze_presets_minutes.iter().enumerate() {
            issues.range(
                &format!("prompt.snooze_presets_minutes[{index}]"),
                *preset,
                min..=max,
            );
        }
    }
}
