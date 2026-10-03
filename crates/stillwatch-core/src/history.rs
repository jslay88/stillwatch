//! Decision history entries.
//!
//! Entries hold numbers and state names only: no pixels, window titles, or
//! media metadata. They are stored one JSON object per line.

use std::time::Duration;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::command::BlankMethod;
use crate::state::State;
use crate::stats::DetectionStats;

/// What kind of decision an entry records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryKind {
    /// A state transition.
    Transition,
    /// The prompt was shown.
    Prompt,
    /// The user snoozed.
    Snooze,
    /// Displays were blanked.
    Blank,
    /// Displays were blanked again after waking without input.
    Reblank,
    /// The snooze or pause ceiling forced a prompt.
    Ceiling,
    /// The config was reloaded.
    ConfigReload,
}

/// The inputs behind a decision, besides detection stats.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct DecisionContext {
    /// A non-ignored media player was playing.
    pub media_playing: bool,
    /// A gamepad passed the deadzone within the idle timeout.
    pub gamepad_active: bool,
    /// The session was locked.
    pub locked: bool,
}

/// One decision history record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// When the decision was made (wall clock).
    pub at: Timestamp,
    /// What was decided.
    pub kind: HistoryKind,
    /// State before a transition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<State>,
    /// State after a transition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<State>,
    /// Detector output behind the decision, when a capture was involved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detection: Option<DetectionStats>,
    /// Blank method used, for `Blank` and `Reblank` entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blank_method: Option<BlankMethod>,
    /// Snooze length in seconds, for `Snooze` entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snooze_seconds: Option<u64>,
    /// Media, gamepad, and lock state at decision time.
    #[serde(flatten)]
    pub context: DecisionContext,
}

impl HistoryEntry {
    /// An entry with no optional details.
    #[must_use]
    pub fn new(at: Timestamp, kind: HistoryKind) -> Self {
        Self {
            at,
            kind,
            from: None,
            to: None,
            detection: None,
            blank_method: None,
            snooze_seconds: None,
            context: DecisionContext::default(),
        }
    }

    /// A [`HistoryKind::Transition`] entry from `from` to `to`.
    #[must_use]
    pub fn transition(at: Timestamp, from: State, to: State) -> Self {
        Self {
            from: Some(from),
            to: Some(to),
            ..Self::new(at, HistoryKind::Transition)
        }
    }

    /// Attaches detector output.
    #[must_use]
    pub fn with_detection(mut self, detection: DetectionStats) -> Self {
        self.detection = Some(detection);
        self
    }

    /// Attaches the blank method.
    #[must_use]
    pub fn with_blank_method(mut self, method: BlankMethod) -> Self {
        self.blank_method = Some(method);
        self
    }

    /// Attaches a snooze length, rounded down to whole seconds.
    #[must_use]
    pub fn with_snooze(mut self, snooze: Duration) -> Self {
        self.snooze_seconds = Some(snooze.as_secs());
        self
    }

    /// Attaches the decision context.
    #[must_use]
    pub fn with_context(mut self, context: DecisionContext) -> Self {
        self.context = context;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::{BlockCounts, OutputStats, Threshold, ThresholdReason};

    fn at() -> Timestamp {
        Timestamp::from_second(1_790_000_000).unwrap()
    }

    #[test]
    fn minimal_entry_serializes_without_empty_options() {
        let json =
            serde_json::to_string(&HistoryEntry::new(at(), HistoryKind::ConfigReload)).unwrap();
        assert_eq!(
            json,
            r#"{"at":"2026-09-21T14:13:20Z","kind":"config_reload","media_playing":false,"gamepad_active":false,"locked":false}"#
        );
    }

    #[test]
    fn full_entry_round_trips() {
        let counts = BlockCounts {
            total: 4,
            counted: 4,
            persistent: 4,
            dark: 0,
            ignored: 0,
        };
        let entry = HistoryEntry::transition(at(), State::Monitoring, State::Prompting)
            .with_detection(DetectionStats {
                outputs: vec![OutputStats::from_counts("HDMI-A-1", counts, 70)],
                threshold: Threshold::new(70, ThresholdReason::Normal),
                stale: true,
            })
            .with_blank_method(BlankMethod::Dpms)
            .with_snooze(Duration::from_millis(900_500))
            .with_context(DecisionContext {
                media_playing: true,
                gamepad_active: false,
                locked: true,
            });
        assert_eq!(entry.snooze_seconds, Some(900));
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains(r#""from":"monitoring","to":"prompting""#));
        assert_eq!(serde_json::from_str::<HistoryEntry>(&json).unwrap(), entry);
    }

    #[test]
    fn missing_context_fields_default_to_false() {
        let entry: HistoryEntry =
            serde_json::from_str(r#"{"at":"2026-09-21T14:13:20Z","kind":"blank"}"#).unwrap();
        assert_eq!(entry, HistoryEntry::new(at(), HistoryKind::Blank));
    }
}
