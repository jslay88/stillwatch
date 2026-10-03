//! The words on the notifications: summaries, bodies, and duration labels.

use std::fmt::Write as _;
use std::time::Duration;

use stillwatch_core::prompt::StaleOutput;

/// Shown when the prompt doesn't say which outputs were static.
const STATIC_FALLBACK: &str = "The screen hasn't changed in a while.";

const SNOOZE_HINT: &str = "Snooze to keep it on.";

/// A short label for a duration: `15 min`, `1 h`, `1 h 30 min`, `50 s`.
/// Zero reads as `0 s`.
pub(crate) fn duration_label(duration: Duration) -> String {
    let total = duration.as_secs();
    let parts = [
        (total / 3600, "h"),
        (total % 3600 / 60, "min"),
        (total % 60, "s"),
    ];
    let label = parts
        .iter()
        .filter(|(value, _)| *value > 0)
        .map(|(value, unit)| format!("{value} {unit}"))
        .collect::<Vec<_>>()
        .join(" ");
    if label.is_empty() {
        "0 s".to_owned()
    } else {
        label
    }
}

/// The prompt's summary line, with `remaining` until the action runs.
pub(crate) fn prompt_summary(remaining: Duration) -> String {
    if remaining.is_zero() {
        "Blanking the screen now".to_owned()
    } else {
        format!("Blanking the screen in {}", duration_label(remaining))
    }
}

/// The prompt's body: which outputs looked static and by how much, then what
/// the user can do about it.
pub(crate) fn prompt_body(stale_outputs: &[StaleOutput]) -> String {
    let mut body = String::new();
    for stale in stale_outputs {
        let _ = writeln!(
            body,
            "{} has been static: {}% of the screen unchanged.",
            stale.output, stale.unchanged_percent
        );
    }
    if body.is_empty() {
        body.push_str(STATIC_FALLBACK);
        body.push('\n');
    }
    body.push_str(SNOOZE_HINT);
    body
}

/// The panel care reminder's summary.
pub(crate) const PANEL_CARE_SUMMARY: &str = "Give the display a rest";

/// The panel care reminder's body, for `screen_on` since the last standby.
pub(crate) fn panel_care_body(screen_on: Duration) -> String {
    let whole_minutes = Duration::from_secs(screen_on.as_secs() / 60 * 60);
    format!(
        "It has been on for {} without a long standby. Turning it off lets the \
         panel run its own care cycle.",
        duration_label(whole_minutes)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_labels_use_the_largest_units() {
        let cases = [
            (Duration::from_mins(15), "15 min"),
            (Duration::from_hours(1), "1 h"),
            (Duration::from_mins(180), "3 h"),
            (Duration::from_mins(90), "1 h 30 min"),
            (Duration::from_secs(50), "50 s"),
            (Duration::from_secs(110), "1 min 50 s"),
            (Duration::from_secs(3601), "1 h 1 s"),
            (Duration::ZERO, "0 s"),
            (Duration::from_millis(900), "0 s"),
        ];
        for (duration, label) in cases {
            assert_eq!(duration_label(duration), label, "{duration:?}");
        }
    }

    #[test]
    fn summary_counts_down_then_says_now() {
        assert_eq!(
            prompt_summary(Duration::from_mins(1)),
            "Blanking the screen in 1 min"
        );
        assert_eq!(
            prompt_summary(Duration::from_secs(40)),
            "Blanking the screen in 40 s"
        );
        assert_eq!(prompt_summary(Duration::ZERO), "Blanking the screen now");
    }

    #[test]
    fn body_names_each_static_output() {
        let stale = [
            StaleOutput {
                output: "HDMI-A-1".into(),
                unchanged_percent: 84,
            },
            StaleOutput {
                output: "DP-1".into(),
                unchanged_percent: 100,
            },
        ];
        assert_eq!(
            prompt_body(&stale),
            "HDMI-A-1 has been static: 84% of the screen unchanged.\n\
             DP-1 has been static: 100% of the screen unchanged.\n\
             Snooze to keep it on."
        );
    }

    #[test]
    fn body_falls_back_to_a_fixed_reason() {
        assert_eq!(
            prompt_body(&[]),
            "The screen hasn't changed in a while.\nSnooze to keep it on."
        );
    }

    #[test]
    fn panel_care_body_rounds_to_minutes() {
        let body = panel_care_body(Duration::from_secs(4 * 3600 + 12 * 60 + 59));
        assert!(body.starts_with("It has been on for 4 h 12 min "), "{body}");
        assert!(body.contains("care cycle"), "{body}");
        assert!(panel_care_body(Duration::from_secs(30)).contains("for 0 s "));
    }
}
