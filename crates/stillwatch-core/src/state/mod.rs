//! The daemon's top-level states.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

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
mod tests {
    use super::*;

    #[test]
    fn names_round_trip_through_from_str_and_serde() {
        for state in State::ALL {
            assert_eq!(state.as_str().parse::<State>(), Ok(state));
            assert_eq!(state.to_string(), state.as_str());
            let json = serde_json::to_string(&state).unwrap();
            assert_eq!(json, format!("\"{}\"", state.as_str()));
            assert_eq!(serde_json::from_str::<State>(&json).unwrap(), state);
        }
    }

    #[test]
    fn unknown_name_is_an_error() {
        let err = "sleeping".parse::<State>().unwrap_err();
        assert_eq!(err, UnknownState("sleeping".into()));
        assert_eq!(err.to_string(), "unknown state name: sleeping");
    }
}
