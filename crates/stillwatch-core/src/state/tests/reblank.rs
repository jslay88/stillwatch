//! The re-blank watchdog and its fallback.

use std::time::Duration;

use super::transitions::reach;
use super::{changed, config, records_of, transition_record};
use crate::command::{BlankMethod, Command};
use crate::config::{ActionMode, Config, ReblankFallback};
use crate::event::Event;
use crate::history::HistoryKind;
use crate::mocks::Harness;
use crate::state::State;
use crate::time::TimerId;

const GRACE: Duration = Duration::from_secs(15);

fn blank(method: BlankMethod) -> Command {
    Command::Blank {
        outputs: vec![],
        method,
    }
}

/// A display wakes, the grace period passes, and the re-blank lands.
/// Returns the commands up to the new `Blank`.
fn wake_and_reblank(h: &mut Harness) -> Vec<Command> {
    let mut commands = h.display_on();
    commands.extend(h.advance(GRACE));
    h.send(Event::ActionCompleted);
    commands
}

fn blanked_with(edit: impl FnOnce(&mut Config)) -> Harness {
    let mut h = Harness::with_config(&config(edit));
    h.to_blanked();
    h
}

#[test]
fn waking_without_input_reblanks_after_the_grace_period() {
    let mut h = reach(State::Blanked);
    assert_eq!(
        h.display_on(),
        vec![Command::SetTimer {
            id: TimerId::ReblankGrace,
            after: GRACE,
        }]
    );
    h.advance(Duration::from_secs(10));
    // More wake reports don't push the re-blank back.
    assert_eq!(h.display_on(), vec![]);
    h.advance(Duration::from_secs(4));
    assert_eq!(h.state(), State::Blanked);

    let commands = h.advance(Duration::from_secs(1));
    assert!(commands.contains(&changed(State::Blanked, State::Acting)));
    assert_eq!(commands.last(), Some(&blank(BlankMethod::Dpms)));
    let reblank = records_of(&commands, HistoryKind::Reblank);
    assert_eq!(reblank.len(), 1);
    assert_eq!(reblank[0].reblank_attempt, Some(1));
    assert_eq!(reblank[0].blank_method, Some(BlankMethod::Dpms));
    assert_eq!(records_of(&commands, HistoryKind::Blank), vec![]);
    let entry = transition_record(&commands);
    assert_eq!(entry.reblank_attempt, Some(1));
    assert_eq!(entry.blank_method, Some(BlankMethod::Dpms));
    assert_eq!(records_of(&commands, HistoryKind::OverlayUsed), vec![]);

    let commands = h.send(Event::ActionCompleted);
    assert_eq!(commands[0], changed(State::Acting, State::Blanked));
    assert!(h.machine().displays_blanked());
}

#[test]
fn after_max_attempts_falls_back_to_the_overlay() {
    let mut h = reach(State::Blanked);
    for attempt in 1..=3 {
        let commands = wake_and_reblank(&mut h);
        assert!(commands.contains(&blank(BlankMethod::Dpms)), "{attempt}");
    }
    for attempt in 4..=5 {
        let commands = wake_and_reblank(&mut h);
        assert!(commands.contains(&blank(BlankMethod::Overlay)), "{attempt}");
        let used = records_of(&commands, HistoryKind::OverlayUsed);
        assert_eq!(used.len(), 1);
        assert_eq!(used[0].reblank_attempt, Some(attempt));
        assert_eq!(used[0].blank_method, Some(BlankMethod::Overlay));
        let json = serde_json::to_string(&used[0]).unwrap();
        assert!(json.contains(r#""kind":"overlay_used","#));
        assert!(json.contains(&format!(r#""reblank_attempt":{attempt}"#)));
    }
    assert_eq!(h.state(), State::Blanked);
    let entry = transition_record(&h.log()[h.log().len() - 2..]);
    assert_eq!(entry.blank_method, Some(BlankMethod::Overlay));
}

#[test]
fn none_fallback_stops_after_max_attempts() {
    let mut h = blanked_with(|c| c.action.reblank_fallback = ReblankFallback::None);
    for _ in 0..3 {
        wake_and_reblank(&mut h);
    }
    assert_eq!(h.display_on(), vec![]);
    assert_eq!(h.advance(GRACE * 10), vec![]);
    assert_eq!(h.state(), State::Blanked);
    // Input still wakes the outputs Stillwatch blanked.
    assert_eq!(h.input()[0], Command::Unblank { outputs: vec![] });
}

#[test]
fn zero_max_attempts_is_unlimited() {
    let mut h = blanked_with(|c| c.action.reblank_max_attempts = 0);
    for attempt in 1..=10 {
        let commands = wake_and_reblank(&mut h);
        assert!(commands.contains(&blank(BlankMethod::Dpms)), "{attempt}");
        let reblank = records_of(&commands, HistoryKind::Reblank);
        assert_eq!(reblank[0].reblank_attempt, Some(attempt));
    }
}

#[test]
fn input_during_grace_cancels_the_reblank() {
    let mut h = reach(State::Blanked);
    h.display_on();
    let commands = h.input();
    assert_eq!(
        commands[..3],
        [
            Command::Unblank { outputs: vec![] },
            Command::CancelTimer(TimerId::ReblankGrace),
            changed(State::Blanked, State::Active)
        ]
    );
    assert!(h.timers().is_empty());
    assert_eq!(h.advance(GRACE), vec![]);
}

#[test]
fn attempts_count_per_blank_episode() {
    let mut h = reach(State::Blanked);
    wake_and_reblank(&mut h);
    wake_and_reblank(&mut h);
    h.input();
    h.to_blanked();
    let commands = wake_and_reblank(&mut h);
    let reblank = records_of(&commands, HistoryKind::Reblank);
    assert_eq!(reblank[0].reblank_attempt, Some(1));
}

#[test]
fn watchdog_off_ignores_wakes() {
    let mut h = blanked_with(|c| c.action.reblank_on_wake = false);
    assert_eq!(h.display_on(), vec![]);
    assert_eq!(h.send(Event::Timer(TimerId::ReblankGrace)), vec![]);
    assert_eq!(h.state(), State::Blanked);
}

#[test]
fn only_outputs_stillwatch_blanked_are_watched() {
    let mut h = blanked_with(|c| c.stale.monitored_outputs = vec!["HDMI-A-1".into()]);
    let other = Event::DisplayPower {
        output: "DP-2".into(),
        on: true,
    };
    assert_eq!(h.send(other), vec![]);
    h.display_on();
    assert!(h.timers().contains(TimerId::ReblankGrace));

    // Command mode blanks nothing itself, so there is nothing to re-blank.
    let mut h = blanked_with(|c| {
        c.action.mode = ActionMode::Command;
        c.action.command = "my-screen-off".into();
    });
    assert_eq!(h.display_on(), vec![]);
}

#[test]
fn a_reblank_only_blanks() {
    let mut h = blanked_with(|c| c.action.mode = ActionMode::LockAndBlank);
    let commands = wake_and_reblank(&mut h);
    assert!(commands.contains(&blank(BlankMethod::Dpms)));
    assert!(!commands.contains(&Command::Lock));
}

#[test]
fn input_during_a_reblank_wakes_the_displays() {
    let mut h = reach(State::Blanked);
    h.display_on();
    h.advance(GRACE);
    assert_eq!(h.state(), State::Acting);
    let commands = h.input();
    assert_eq!(commands[0], Command::Unblank { outputs: vec![] });
    assert_eq!(h.state(), State::Active);
}
