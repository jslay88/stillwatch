//! Decision history entries.
//!
//! Entries hold numbers and state names only: no pixels, window titles, or
//! media metadata. They are stored one JSON object per line.

use std::time::Duration;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::command::BlankMethod;
use crate::prompt::PromptOutcome;
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
    /// The re-blank watchdog fell back to the black overlay.
    OverlayUsed,
    /// The prompt ended; `answer` says how.
    PromptAnswered,
    /// A config reload failed and the last good config stayed in effect.
    ConfigReloadFailed,
    /// The loaded config was migrated in memory from an older version.
    Migration,
}

/// How a prompt ended, for [`HistoryKind::PromptAnswered`] entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptAnswer {
    /// A snooze was picked; the length is in `snooze_seconds`.
    Snooze,
    /// "Custom..." was picked; the real answer follows later.
    Custom,
    /// The user cancelled.
    Cancel,
    /// The countdown ran out (the prompter's or Stillwatch's own).
    Timeout,
    /// The prompt was closed without an action.
    Dismissed,
    /// The prompt couldn't be shown or failed while showing.
    Failed,
}

impl From<PromptOutcome> for PromptAnswer {
    fn from(outcome: PromptOutcome) -> Self {
        match outcome {
            PromptOutcome::Snooze(_) => Self::Snooze,
            PromptOutcome::CustomRequested => Self::Custom,
            PromptOutcome::Cancel => Self::Cancel,
            PromptOutcome::Timeout => Self::Timeout,
            PromptOutcome::Dismissed => Self::Dismissed,
        }
    }
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
    /// Re-blank attempt within the current blank episode, starting at 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reblank_attempt: Option<u32>,
    /// How the prompt ended, for `PromptAnswered` entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<PromptAnswer>,
    /// Number of problems found, for `ConfigReloadFailed` entries. The
    /// messages themselves are never recorded, since they can contain paths.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_count: Option<u32>,
    /// Config version migrated from, for `Migration` entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_version: Option<u32>,
    /// Config version migrated to, for `Migration` entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_version: Option<u32>,
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
            reblank_attempt: None,
            answer: None,
            error_count: None,
            from_version: None,
            to_version: None,
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

    /// Attaches the re-blank attempt number.
    #[must_use]
    pub fn with_reblank_attempt(mut self, attempt: u32) -> Self {
        self.reblank_attempt = Some(attempt);
        self
    }

    /// Attaches the decision context.
    #[must_use]
    pub fn with_context(mut self, context: DecisionContext) -> Self {
        self.context = context;
        self
    }

    /// Attaches how the prompt ended.
    #[must_use]
    pub const fn with_answer(mut self, answer: PromptAnswer) -> Self {
        self.answer = Some(answer);
        self
    }

    /// Attaches the number of problems a failed reload found.
    #[must_use]
    pub const fn with_error_count(mut self, count: u32) -> Self {
        self.error_count = Some(count);
        self
    }

    /// Attaches the config versions a migration went between.
    #[must_use]
    pub const fn with_versions(mut self, from: u32, to: u32) -> Self {
        self.from_version = Some(from);
        self.to_version = Some(to);
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

#[cfg(test)]
mod privacy;
