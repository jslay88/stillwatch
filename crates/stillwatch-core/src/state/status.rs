use std::time::Duration;

use super::State;
use crate::stats::DetectionStats;

/// What the machine reports for the D-Bus `Status()` call.
///
/// The daemon maps it onto `stillwatch_ipc::status::StatusPayload` and adds
/// what only it knows (backends in use, config errors, panel care).
#[derive(Debug, Clone, PartialEq)]
pub struct StatusSnapshot {
    /// Current state.
    pub state: State,
    /// Time spent in the current state.
    pub in_state: Duration,
    /// Time until the snooze ends, while snoozed.
    pub snooze_remaining: Option<Duration>,
    /// Whether the user is idle.
    pub idle: bool,
    /// Whether the session is locked.
    pub locked: bool,
    /// Whether a non-ignored media player is playing.
    pub media_playing: bool,
    /// The most recent detector verdict.
    pub last_detection: Option<DetectionStats>,
}
