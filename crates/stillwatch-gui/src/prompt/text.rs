//! Words on the prompt dialog, and the custom-duration parser.

use std::fmt::Write as _;

use stillwatch_core::prompt::StaleOutput;
use stillwatch_core::state::State;
use stillwatch_ipc::status::StatusPayload;

/// Shown until a status names a static output.
pub(crate) const STATIC_FALLBACK: &str = "The screen looks static.";

/// Why the prompt appeared, from the daemon's last detection.
#[must_use]
pub(crate) fn static_summary(status: &StatusPayload) -> String {
    let Some(detection) = &status.last_detection else {
        return STATIC_FALLBACK.to_owned();
    };
    let stale = StaleOutput::from_detection(detection);
    if stale.is_empty() {
        return STATIC_FALLBACK.to_owned();
    }
    let mut lines = String::new();
    for (index, output) in stale.iter().enumerate() {
        if index > 0 {
            lines.push('\n');
        }
        let _ = write!(
            lines,
            "{} is {}% unchanged",
            output.output, output.unchanged_percent
        );
    }
    lines
}

/// Countdown line. `0` reads as blanking now.
#[must_use]
pub(crate) fn countdown_line(secs: u64) -> String {
    if secs == 0 {
        "Blanking now".to_owned()
    } else {
        format!("Blanking in {secs}s")
    }
}

/// A preset button label.
#[must_use]
pub(crate) fn preset_label(minutes: u32) -> String {
    if minutes >= 60 && minutes.is_multiple_of(60) {
        let hours = minutes / 60;
        if hours == 1 {
            "1 hour".to_owned()
        } else {
            format!("{hours} hours")
        }
    } else {
        format!("{minutes} min")
    }
}

/// Seconds left when `--remaining` was not passed.
///
/// While the daemon is prompting, that is `countdown` minus time already
/// spent in the state. Otherwise it is the configured countdown.
#[must_use]
pub(crate) fn remaining_secs(status: &StatusPayload, countdown: u32) -> u64 {
    let countdown = u64::from(countdown);
    if status.state == State::Prompting {
        countdown.saturating_sub(status.state_seconds)
    } else {
        countdown
    }
}

/// Parses a custom snooze into whole minutes inside `min..=max`.
///
/// A bare number is minutes (`45` and `45m` are the same). Anything
/// [`humantime`] understands is accepted when it lands on a whole minute.
///
/// # Errors
///
/// Returns a short message when the text isn't a duration or falls outside
/// `min..=max`.
pub(crate) fn parse_custom(text: &str, min: u32, max: u32) -> Result<u32, String> {
    let text = text.trim();
    let hint = format!("enter a duration like 45m ({min}-{max} min)");
    if text.is_empty() {
        return Err(hint);
    }
    let parsed = if text.bytes().all(|byte| byte.is_ascii_digit()) {
        humantime::parse_duration(&format!("{text}m"))
    } else {
        humantime::parse_duration(text)
    };
    let duration = parsed.map_err(|_| hint)?;
    let secs = duration.as_secs();
    if secs == 0 || duration.subsec_nanos() != 0 || !secs.is_multiple_of(60) {
        return Err("use a whole number of minutes".to_owned());
    }
    let minutes = u32::try_from(secs / 60).unwrap_or(u32::MAX);
    if minutes < min {
        return Err(format!("at least {min} min"));
    }
    if minutes > max {
        return Err(format!("at most {max} min"));
    }
    Ok(minutes)
}

#[cfg(test)]
mod tests;
