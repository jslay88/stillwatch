mod blank_path;
mod gamepad;
mod machine;
mod snooze;
mod table;
mod transitions;

use super::*;
use crate::command::Command;
use crate::history::HistoryEntry;

/// The `Record` entries among `commands`.
fn records(commands: &[Command]) -> Vec<HistoryEntry> {
    commands
        .iter()
        .filter_map(|command| match command {
            Command::Record(entry) => Some(entry.clone()),
            _ => None,
        })
        .collect()
}

/// The single transition entry among `commands`.
fn transition_record(commands: &[Command]) -> HistoryEntry {
    let mut entries: Vec<_> = records(commands)
        .into_iter()
        .filter(|entry| entry.kind == crate::history::HistoryKind::Transition)
        .collect();
    assert_eq!(entries.len(), 1, "expected one transition in {commands:?}");
    entries.remove(0)
}

fn changed(from: State, to: State) -> Command {
    Command::StateChanged { from, to }
}

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
