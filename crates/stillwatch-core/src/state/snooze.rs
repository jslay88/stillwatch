use std::time::Duration;

use crate::config::PromptConfig;

/// Why a snooze duration was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SnoozeError {
    /// Not a preset, and `prompt.allow_custom` is off.
    #[error("custom snooze durations are disabled, pick a preset ({presets:?} minutes)")]
    CustomDisabled {
        /// The offered presets, in minutes.
        presets: Vec<u32>,
    },
    /// Shorter than `prompt.custom_min_minutes`.
    #[error("snooze must be at least {min_minutes} minutes")]
    TooShort {
        /// `prompt.custom_min_minutes`.
        min_minutes: u32,
    },
    /// Longer than `prompt.custom_max_minutes`.
    #[error("snooze must be at most {max_minutes} minutes")]
    TooLong {
        /// `prompt.custom_max_minutes`.
        max_minutes: u32,
    },
}

/// Checks a requested snooze against the `[prompt]` rules.
///
/// A preset is always accepted. Anything else needs `allow_custom` and must
/// fall within `custom_min_minutes..=custom_max_minutes`.
///
/// # Errors
///
/// Returns the [`SnoozeError`] describing the rule `duration` breaks.
pub fn validate_snooze(prompt: &PromptConfig, duration: Duration) -> Result<Duration, SnoozeError> {
    if prompt_presets(prompt).any(|preset| preset == duration) {
        return Ok(duration);
    }
    if !prompt.allow_custom {
        return Err(SnoozeError::CustomDisabled {
            presets: prompt.snooze_presets_minutes.clone(),
        });
    }
    if duration < minutes(prompt.custom_min_minutes) {
        return Err(SnoozeError::TooShort {
            min_minutes: prompt.custom_min_minutes,
        });
    }
    if duration > minutes(prompt.custom_max_minutes) {
        return Err(SnoozeError::TooLong {
            max_minutes: prompt.custom_max_minutes,
        });
    }
    Ok(duration)
}

/// The snooze presets as durations, in display order.
pub(super) fn prompt_presets(prompt: &PromptConfig) -> impl Iterator<Item = Duration> + '_ {
    prompt.snooze_presets_minutes.iter().map(|m| minutes(*m))
}

fn minutes(m: u32) -> Duration {
    Duration::from_mins(u64::from(m))
}
