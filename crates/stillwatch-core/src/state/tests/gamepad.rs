//! Gamepad input in each state.

use std::time::Duration;

use super::changed;
use super::transitions::reach;
use crate::command::Command;
use crate::config::Config;
use crate::event::ControlCommand;
use crate::mocks::Harness;
use crate::state::State;

fn config(edit: impl FnOnce(&mut Config)) -> Config {
    let mut config = Config::default();
    edit(&mut config);
    config
}

#[test]
fn gamepad_in_monitoring_counts_as_input() {
    let mut h = reach(State::Monitoring);
    let commands = h.gamepad();
    assert!(commands.contains(&changed(State::Monitoring, State::Active)));
    assert!(h.timers().is_empty());
}

#[test]
fn gamepad_in_prompting_counts_as_input() {
    let mut h = reach(State::Prompting);
    let commands = h.gamepad();
    assert!(commands.contains(&Command::DismissPrompt));
    assert!(commands.contains(&changed(State::Prompting, State::Active)));
}

#[test]
fn gamepad_in_blanked_wakes_the_displays() {
    let mut h = reach(State::Blanked);
    let commands = h.gamepad();
    assert_eq!(
        commands[..2],
        [
            Command::Unblank { outputs: vec![] },
            changed(State::Blanked, State::Active)
        ]
    );
}

#[test]
fn gamepad_in_blanked_without_wakes_display_leaves_displays_to_the_keyboard() {
    let mut h = Harness::with_config(&config(|c| c.activity.gamepad_wakes_display = false));
    h.to_blanked();
    let commands = h.gamepad();
    assert_eq!(commands[0], changed(State::Blanked, State::Active));
    assert!(
        !commands
            .iter()
            .any(|c| matches!(c, Command::Unblank { .. }))
    );
    assert!(h.machine().displays_blanked());

    let commands = h.input();
    assert_eq!(commands, vec![Command::Unblank { outputs: vec![] }]);
    assert!(!h.machine().displays_blanked());
}

#[test]
fn gamepad_in_acting_counts_as_input() {
    let mut h = reach(State::Acting);
    let commands = h.gamepad();
    assert!(commands.contains(&changed(State::Acting, State::Active)));
}

#[test]
fn gamepad_in_snoozed_follows_snooze_cancelled_by_input() {
    let mut h = reach(State::Snoozed);
    assert_eq!(h.gamepad(), vec![]);
    assert_eq!(h.state(), State::Snoozed);

    let mut h = Harness::with_config(&config(|c| c.prompt.snooze_cancelled_by_input = true));
    h.send(ControlCommand::Snooze(Duration::from_mins(15)));
    h.gamepad();
    assert_eq!(h.state(), State::Active);
}

#[test]
fn gamepad_in_active_paused_and_locked_changes_nothing() {
    for state in [State::Active, State::Paused, State::Locked] {
        let mut h = reach(state);
        assert_eq!(h.gamepad(), vec![], "{state}");
        assert_eq!(h.state(), state);
    }
}

#[test]
fn gamepad_events_are_dropped_when_gamepad_is_off() {
    let mut h = Harness::with_config(&config(|c| c.activity.gamepad = false));
    h.idle();
    assert_eq!(h.gamepad(), vec![]);
    assert_eq!(h.state(), State::Monitoring);
    assert!(h.status().idle);
}

#[test]
fn recent_gamepad_input_is_in_the_decision_context() {
    let mut h = Harness::new();
    h.gamepad();
    let entry = super::transition_record(&h.idle());
    assert!(entry.context.gamepad_active);

    h.input();
    h.advance(Duration::from_mins(10));
    let entry = super::transition_record(&h.idle());
    assert!(!entry.context.gamepad_active);
}
