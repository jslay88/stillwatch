//! The transition table matches what the machine does.

use std::collections::BTreeSet;
use std::time::Duration;

use super::transitions::reach;
use crate::backend::BackendError;
use crate::config::Config;
use crate::event::{ControlCommand, Event, SessionEvent};
use crate::mocks::{Harness, ScriptedDetector};
use crate::prompt::PromptOutcome;
use crate::state::context::Transition;
use crate::state::{State, StateMachine, TRANSITIONS, rule_for};
use crate::time::{Clock, FakeClock, TimerId};

type Step = fn(&mut Harness);

fn snooze(h: &mut Harness) {
    h.send(ControlCommand::Snooze(Duration::from_mins(15)));
}

fn lock(h: &mut Harness) {
    h.send(SessionEvent::Locked);
}

fn pause(h: &mut Harness) {
    h.send(ControlCommand::Pause);
}

const SCENARIOS: &[(State, Step)] = &[
    (State::Active, |h| {
        h.idle();
    }),
    (State::Active, snooze),
    (State::Active, lock),
    (State::Monitoring, |h| {
        h.input();
    }),
    (State::Monitoring, |h| {
        h.capture(true);
    }),
    (State::Monitoring, snooze),
    (State::Monitoring, lock),
    (State::Prompting, |h| {
        h.answer(PromptOutcome::Snooze(Duration::from_hours(1)));
    }),
    (State::Prompting, |h| {
        h.answer(PromptOutcome::Cancel);
    }),
    (State::Prompting, |h| {
        h.fire(TimerId::PromptCountdown);
    }),
    (State::Acting, |h| {
        h.send(Event::ActionCompleted);
    }),
    (State::Acting, |h| {
        h.input();
    }),
    (State::Acting, |h| {
        h.send(Event::ActionFailed {
            error: BackendError::Io("x".into()),
        });
    }),
    (State::Blanked, |h| {
        h.input();
    }),
    (State::Snoozed, |h| {
        h.send(ControlCommand::CancelSnooze);
    }),
    (State::Snoozed, |h| {
        h.input();
        h.send(ControlCommand::CancelSnooze);
    }),
    (State::Locked, |h| {
        h.send(SessionEvent::Unlocked);
    }),
    (State::Active, pause),
    (State::Monitoring, pause),
    (State::Prompting, pause),
    (State::Snoozed, pause),
    (State::Acting, pause),
    (State::Blanked, pause),
    (State::Locked, pause),
    (State::Paused, |h| {
        h.send(ControlCommand::Resume);
    }),
    (State::Paused, |h| {
        h.idle();
        h.send(ControlCommand::Resume);
    }),
    (State::Snoozed, |h| {
        h.capture_ceiling(true);
    }),
    (State::Paused, |h| {
        let mut config = Config::default();
        config.safety.ceiling_during_pause = true;
        h.apply_config(&config);
        h.idle();
        h.capture_ceiling(true);
    }),
    (State::Locked, |h| {
        h.fire(TimerId::LockedBlank);
    }),
    (State::Blanked, |h| {
        h.display_on();
        h.fire(TimerId::ReblankGrace);
    }),
    (State::Blanked, sleep),
    (State::Snoozed, sleep),
    (State::Blanked, |h| {
        lock(h);
        h.input();
    }),
];

fn sleep(h: &mut Harness) {
    h.send(SessionEvent::PrepareForSleep);
}

#[test]
fn every_row_is_exercised_and_nothing_else_happens() {
    let mut seen = BTreeSet::new();
    for (start, step) in SCENARIOS {
        let mut h = reach(*start);
        let before = h.transitions().len();
        step(&mut h);
        let taken = h.transitions().split_off(before);
        assert!(!taken.is_empty(), "scenario from {start} did nothing");
        for (from, to) in taken {
            assert!(rule_for(from, to).is_some(), "undocumented {from} -> {to}");
            seen.insert((from.as_str(), to.as_str()));
        }
    }
    let documented: BTreeSet<_> = TRANSITIONS
        .iter()
        .map(|rule| (rule.from.as_str(), rule.to.as_str()))
        .collect();
    assert_eq!(seen, documented);
}

#[test]
fn table_rows_are_unique_and_never_self_loops() {
    let unique: BTreeSet<_> = TRANSITIONS
        .iter()
        .map(|r| (r.from.as_str(), r.to.as_str()))
        .collect();
    assert_eq!(unique.len(), TRANSITIONS.len());
    assert!(
        TRANSITIONS
            .iter()
            .all(|r| r.from != r.to && !r.trigger.is_empty())
    );
    assert_eq!(
        rule_for(State::Active, State::Monitoring).map(|r| r.trigger),
        Some("idle")
    );
    assert_eq!(rule_for(State::Active, State::Blanked), None);
}

#[test]
fn undocumented_transitions_are_refused() {
    let clock = FakeClock::new();
    let detector = Box::new(ScriptedDetector::new());
    let (mut machine, _) = StateMachine::new(&Config::default(), detector, clock.now());
    machine.go(Transition::to(State::Blanked));
    assert_eq!(machine.state(), State::Active);
    let commands = machine.handle(
        clock.now(),
        clock.wall_now(),
        &Event::Timer(TimerId::Capture),
    );
    assert_eq!(commands, vec![]);
}
