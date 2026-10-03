//! What the state machine asks a prompter to show, and what comes back.

use std::time::Duration;

use crate::stats::DetectionStats;

/// A request to show the burn-in prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptRequest {
    /// How long until the action runs if nobody answers.
    pub countdown: Duration,
    /// Snooze presets offered as buttons, in display order.
    pub presets: Vec<Duration>,
    /// Whether to offer "Custom..." (which opens `stillwatch-gui prompt`).
    pub allow_custom: bool,
    /// The outputs whose staleness raised the prompt, so it can say why.
    /// Empty when the triggering detection isn't known.
    pub stale_outputs: Vec<StaleOutput>,
}

/// An output that met the stale threshold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleOutput {
    /// Connector name, for example `HDMI-A-1`.
    pub output: String,
    /// Percent of the output's counted blocks that were unchanged, rounded
    /// down.
    pub unchanged_percent: u8,
}

impl StaleOutput {
    /// The stale outputs of a detection, in its order.
    #[must_use]
    pub fn from_detection(detection: &DetectionStats) -> Vec<Self> {
        detection
            .outputs
            .iter()
            .filter(|stats| stats.stale)
            .map(|stats| Self {
                output: stats.output.clone(),
                unchanged_percent: whole_percent(stats.persistent_percent),
            })
            .collect()
    }
}

/// `value` rounded down to a whole percent in 0..=100. NaN reads as 0.
fn whole_percent(value: f64) -> u8 {
    (0..=100_u8)
        .rev()
        .find(|percent| f64::from(*percent) <= value)
        .unwrap_or(0)
}

/// How a prompt ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptOutcome {
    /// The user snoozed for this long (a preset or a custom value).
    Snooze(Duration),
    /// The user picked "Custom...". The prompt stays logically open; the real
    /// answer arrives later through the D-Bus `PromptAnswer` method.
    CustomRequested,
    /// The user cancelled: they're here, don't act.
    Cancel,
    /// Act now ("Blank now"). Prompters must not send this for their own
    /// countdown; the state machine owns the timeout.
    Timeout,
    /// The prompt was closed without picking an action.
    Dismissed,
}

/// A non-blocking informational notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reminder {
    /// Screen-on time passed `reminder_hours`; turning the display off lets
    /// the panel's compensation cycle run.
    PanelCare {
        /// Accumulated screen-on time since the last long enough standby.
        screen_on: Duration,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::{BlockCounts, OutputStats, Threshold, ThresholdReason};

    fn output(name: &str, persistent: u32, counted: u32) -> OutputStats {
        let counts = BlockCounts {
            total: 100,
            counted,
            persistent,
            dark: 100 - counted,
            ignored: 0,
        };
        OutputStats::from_counts(name, counts, 70)
    }

    #[test]
    fn stale_outputs_keep_only_outputs_over_the_threshold() {
        let detection = DetectionStats {
            outputs: vec![
                output("HDMI-A-1", 84, 100),
                output("DP-1", 10, 100),
                output("DP-2", 7, 9),
            ],
            threshold: Threshold::new(70, ThresholdReason::Normal),
            stale: false,
        };
        assert_eq!(
            StaleOutput::from_detection(&detection),
            vec![
                StaleOutput {
                    output: "HDMI-A-1".into(),
                    unchanged_percent: 84,
                },
                StaleOutput {
                    output: "DP-2".into(),
                    unchanged_percent: 77,
                },
            ]
        );
    }

    #[test]
    fn whole_percent_rounds_down_and_clamps() {
        assert_eq!(whole_percent(100.0), 100);
        assert_eq!(whole_percent(250.0), 100);
        assert_eq!(whole_percent(99.99), 99);
        assert_eq!(whole_percent(0.5), 0);
        assert_eq!(whole_percent(-3.0), 0);
        assert_eq!(whole_percent(f64::NAN), 0);
    }
}
