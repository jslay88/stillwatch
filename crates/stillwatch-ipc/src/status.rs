//! The `Status()` payload.

use std::time::Duration;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use stillwatch_core::state::{State, StatusSnapshot};
use stillwatch_core::stats::DetectionStats;

/// Panel care tracking, for status and the GUI.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelCareStatus {
    /// Screen-on time since the last standby of at least `min_standby_minutes`.
    pub screen_on_seconds: u64,
    /// When that standby ended, if one was ever recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_standby: Option<Timestamp>,
    /// How many times the overlay was used instead of real standby.
    pub overlay_uses: u32,
}

/// What `Status()` returns, as JSON.
///
/// Optional and list fields default when absent, so older clients and daemons
/// interoperate as fields are added.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatusPayload {
    /// Current state.
    pub state: State,
    /// Seconds spent in the current state.
    pub state_seconds: u64,
    /// Seconds until the snooze ends, while snoozed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snooze_remaining_seconds: Option<u64>,
    /// Whether the user is idle (input idle and no recent gamepad activity).
    #[serde(default)]
    pub idle: bool,
    /// Whether the session is locked.
    #[serde(default)]
    pub locked: bool,
    /// Whether a non-ignored media player is playing.
    #[serde(default)]
    pub media_playing: bool,
    /// The active capture backend (`kwin`, `portal`), or none in
    /// input-idle-only mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture_backend: Option<String>,
    /// The most recent detector verdict.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_detection: Option<DetectionStats>,
    /// Errors from the last failed reload; empty when the config is good.
    #[serde(default)]
    pub config_errors: Vec<String>,
    /// Panel care tracking, when enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub panel_care: Option<PanelCareStatus>,
}

impl StatusPayload {
    /// A status in `state` with every other field empty.
    #[must_use]
    pub fn new(state: State) -> Self {
        Self {
            state,
            state_seconds: 0,
            snooze_remaining_seconds: None,
            idle: false,
            locked: false,
            media_playing: false,
            capture_backend: None,
            last_detection: None,
            config_errors: Vec::new(),
            panel_care: None,
        }
    }

    /// The state machine's part of the status. Backends, config errors, and
    /// panel care stay empty for the daemon to fill in.
    ///
    /// Snooze time left rounds up, so a running snooze never reads as 0.
    #[must_use]
    pub fn from_snapshot(snapshot: &StatusSnapshot) -> Self {
        Self {
            state_seconds: snapshot.in_state.as_secs(),
            snooze_remaining_seconds: snapshot.snooze_remaining.map(ceil_secs),
            idle: snapshot.idle,
            locked: snapshot.locked,
            media_playing: snapshot.media_playing,
            last_detection: snapshot.last_detection.clone(),
            ..Self::new(snapshot.state)
        }
    }
}

fn ceil_secs(duration: Duration) -> u64 {
    duration
        .as_secs()
        .saturating_add(u64::from(duration.subsec_nanos() > 0))
}

#[cfg(test)]
mod tests {
    use stillwatch_core::stats::{BlockCounts, OutputStats, Threshold, ThresholdReason};

    use super::*;
    use crate::json::{from_json, to_json};

    #[test]
    fn snapshot_maps_onto_the_payload() {
        let detection = DetectionStats {
            outputs: Vec::new(),
            threshold: Threshold::new(70, ThresholdReason::Normal),
            stale: true,
        };
        let snapshot = StatusSnapshot {
            state: State::Snoozed,
            in_state: Duration::from_millis(42_900),
            snooze_remaining: Some(Duration::from_millis(599_001)),
            idle: true,
            locked: true,
            media_playing: true,
            last_detection: Some(detection.clone()),
        };
        assert_eq!(
            StatusPayload::from_snapshot(&snapshot),
            StatusPayload {
                state_seconds: 42,
                snooze_remaining_seconds: Some(600),
                idle: true,
                locked: true,
                media_playing: true,
                last_detection: Some(detection),
                ..StatusPayload::new(State::Snoozed)
            }
        );
    }

    #[test]
    fn whole_seconds_of_snooze_stay_exact() {
        let snapshot = StatusSnapshot {
            state: State::Active,
            in_state: Duration::ZERO,
            snooze_remaining: Some(Duration::from_secs(60)),
            idle: false,
            locked: false,
            media_playing: false,
            last_detection: None,
        };
        let payload = StatusPayload::from_snapshot(&snapshot);
        assert_eq!(payload.snooze_remaining_seconds, Some(60));
        assert_eq!(
            StatusPayload::from_snapshot(&StatusSnapshot {
                snooze_remaining: None,
                ..snapshot
            }),
            StatusPayload::new(State::Active)
        );
    }

    #[test]
    fn full_status_round_trips() {
        let counts = BlockCounts {
            total: 256,
            counted: 200,
            persistent: 150,
            dark: 56,
            ignored: 0,
        };
        let status = StatusPayload {
            state_seconds: 42,
            snooze_remaining_seconds: Some(600),
            idle: true,
            media_playing: true,
            capture_backend: Some("kwin".into()),
            last_detection: Some(DetectionStats {
                outputs: vec![OutputStats::from_counts("HDMI-A-1", counts, 90)],
                threshold: Threshold::new(90, ThresholdReason::Media),
                stale: false,
            }),
            config_errors: vec!["stale.stale_percent: must be 1-100".into()],
            panel_care: Some(PanelCareStatus {
                screen_on_seconds: 3 * 3600,
                last_standby: Some(Timestamp::from_second(1_790_000_000).unwrap()),
                overlay_uses: 2,
            }),
            ..StatusPayload::new(State::Snoozed)
        };
        let json = to_json(&status).unwrap();
        assert_eq!(from_json::<StatusPayload>(&json).unwrap(), status);
    }

    #[test]
    fn minimal_status_needs_only_state_and_time() {
        let status: StatusPayload =
            from_json(r#"{"state":"monitoring","state_seconds":5}"#).unwrap();
        assert_eq!(
            status,
            StatusPayload {
                state_seconds: 5,
                ..StatusPayload::new(State::Monitoring)
            }
        );
        let json = to_json(&StatusPayload::new(State::Active)).unwrap();
        assert!(!json.contains("snooze_remaining_seconds"));
    }
}
