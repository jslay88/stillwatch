//! JSON encoding for payloads carried as D-Bus strings.
//!
//! `Status`, `Gamepads`, and the `ProbeSample` signal carry one JSON
//! document; `History` carries JSON lines (one object per line, each ending in
//! `\n`).

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::IpcError;

/// Encodes one value as a compact JSON document.
///
/// # Errors
///
/// Returns [`IpcError::Json`] if `value` can't be represented as JSON.
pub fn to_json<T: Serialize + ?Sized>(value: &T) -> Result<String, IpcError> {
    Ok(serde_json::to_string(value)?)
}

/// Decodes one JSON document.
///
/// # Errors
///
/// Returns [`IpcError::Json`] if `json` isn't a valid `T`.
pub fn from_json<T: DeserializeOwned>(json: &str) -> Result<T, IpcError> {
    Ok(serde_json::from_str(json)?)
}

/// Encodes values as JSON lines.
///
/// # Errors
///
/// Returns [`IpcError::Json`] if a value can't be represented as JSON.
pub fn to_json_lines<T: Serialize>(values: &[T]) -> Result<String, IpcError> {
    let mut out = String::new();
    for value in values {
        out.push_str(&to_json(value)?);
        out.push('\n');
    }
    Ok(out)
}

/// Decodes JSON lines, skipping blank lines.
///
/// # Errors
///
/// Returns [`IpcError::Json`] for the first line that isn't a valid `T`.
pub fn from_json_lines<T: DeserializeOwned>(lines: &str) -> Result<Vec<T>, IpcError> {
    lines
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(from_json)
        .collect()
}

#[cfg(test)]
mod tests {
    use jiff::Timestamp;
    use stillwatch_core::history::{HistoryEntry, HistoryKind};
    use stillwatch_core::state::State;

    use super::*;

    fn entries() -> Vec<HistoryEntry> {
        let at = Timestamp::from_second(1_790_000_000).unwrap();
        vec![
            HistoryEntry::transition(at, State::Active, State::Monitoring),
            HistoryEntry::new(at, HistoryKind::Prompt),
        ]
    }

    #[test]
    fn single_documents_round_trip() {
        let json = to_json(&State::Blanked).unwrap();
        assert_eq!(json, r#""blanked""#);
        assert_eq!(from_json::<State>(&json).unwrap(), State::Blanked);
    }

    #[test]
    fn history_round_trips_as_json_lines() {
        let lines = to_json_lines(&entries()).unwrap();
        assert_eq!(lines.lines().count(), 2);
        assert!(lines.ends_with('\n'));
        let padded = format!("\n{lines}\n  \n");
        assert_eq!(from_json_lines::<HistoryEntry>(&padded).unwrap(), entries());
    }

    #[test]
    fn bad_json_is_an_error() {
        let err = from_json::<State>("\"asleep\"").unwrap_err();
        assert!(err.to_string().starts_with("invalid JSON payload:"));
        assert!(from_json_lines::<State>("\"active\"\nnope\n").is_err());
    }
}
