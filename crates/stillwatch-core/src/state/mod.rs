//! The state machine: the daemon's top-level states, the transition table,
//! and the sans-IO [`StateMachine`] that walks it.
//!
//! Per-state behavior lives in one handler per state (`handlers/`), events
//! every state treats alike in `handlers/common.rs`, and the facts handlers
//! share (idle, lock, media, detector, blanked outputs) in `context.rs`.
//! A new behavior is a row in [`TRANSITIONS`] plus the handler code that
//! returns it.

mod context;
mod detector;
mod handlers;
mod machine;
mod snooze;
mod status;
mod transitions;

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

pub use detector::StaleDetector;
pub use machine::StateMachine;
pub use snooze::{SnoozeError, validate_snooze};
pub use status::StatusSnapshot;
pub use transitions::{TRANSITIONS, TransitionRule, rule_for};

/// A state of the Stillwatch state machine.
///
/// Serialized as its lowercase name (`"active"`, `"monitoring"`, ...), which is
/// also what [`State::as_str`] returns and what the D-Bus `StateChanged` signal
/// carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// The user is present. No captures run.
    Active,
    /// The user is idle. Captures run every check interval.
    Monitoring,
    /// The screen is stale and the prompt is shown with a countdown.
    Prompting,
    /// The user snoozed. Normal prompting waits for expiry; the ceiling still applies.
    Snoozed,
    /// The configured action is running.
    Acting,
    /// Displays are blanked and the re-blank watchdog is armed.
    Blanked,
    /// The session is locked.
    Locked,
    /// The user paused Stillwatch.
    Paused,
}

impl State {
    /// Every state, in declaration order.
    pub const ALL: [Self; 8] = [
        Self::Active,
        Self::Monitoring,
        Self::Prompting,
        Self::Snoozed,
        Self::Acting,
        Self::Blanked,
        Self::Locked,
        Self::Paused,
    ];

    /// The lowercase name used on the wire and in history.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Monitoring => "monitoring",
            Self::Prompting => "prompting",
            Self::Snoozed => "snoozed",
            Self::Acting => "acting",
            Self::Blanked => "blanked",
            Self::Locked => "locked",
            Self::Paused => "paused",
        }
    }
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Error returned when parsing an unknown state name.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown state name: {0}")]
pub struct UnknownState(pub String);

impl FromStr for State {
    type Err = UnknownState;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|state| state.as_str() == s)
            .ok_or_else(|| UnknownState(s.to_owned()))
    }
}

#[cfg(test)]
mod tests;
