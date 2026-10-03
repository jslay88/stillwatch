//! Suspend and resume from sleep.

use std::time::Duration;

use super::transitions::reach;
use super::{changed, config};
use crate::command::Command;
use crate::config::WhenLocked;
use crate::event::{ControlCommand, Event, SessionEvent};
use crate::mocks::Harness;
use crate::state::State;
use crate::time::TimerId;

fn sleep(h: &mut Harness) -> Vec<Command> {
    h.send(SessionEvent::PrepareForSleep)
}

fn wake(h: &mut Harness) -> Vec<Command> {
    h.send(SessionEvent::ResumedFromSleep)
}

#[test]
fn resume_from_suspend_restarts_from_active() {
    let mut h = reach(State::Monitoring);
    let commands = sleep(&mut h);
    assert_eq!(
        commands[..2],
        [
            Command::CancelTimer(TimerId::Capture),
            changed(State::Monitoring, State::Active)
        ]
    );
    assert!(h.timers().is_empty());

    // Nothing but the resume gets through while asleep.
    assert_eq!(h.idle(), vec![]);
    assert_eq!(
        h.send(ControlCommand::Snooze(Duration::from_mins(15))),
        vec![]
    );
    assert_eq!(sleep(&mut h), vec![]);
    assert_eq!(h.state(), State::Active);

    let resets = h.detector().resets();
    assert_eq!(wake(&mut h), vec![]);
    assert_eq!(h.detector().resets(), resets + 1);
    assert!(!h.status().idle);
    assert_eq!(wake(&mut h), vec![], "a second resume is ignored");

    h.idle();
    assert_eq!(h.state(), State::Monitoring);
}

#[test]
fn every_busy_state_waits_out_the_sleep_in_active() {
    for state in [
        State::Monitoring,
        State::Prompting,
        State::Snoozed,
        State::Acting,
        State::Blanked,
    ] {
        let mut h = reach(state);
        let commands = sleep(&mut h);
        assert!(commands.contains(&changed(state, State::Active)), "{state}");
        assert!(h.timers().is_empty(), "{state}: {:?}", h.timers());
        wake(&mut h);
        assert_eq!(h.state(), State::Active, "{state}");
    }
}

#[test]
fn suspend_while_prompting_dismisses_the_prompt() {
    let mut h = reach(State::Prompting);
    assert!(sleep(&mut h).contains(&Command::DismissPrompt));
    assert_eq!(h.status().snooze_remaining, None);
}

#[test]
fn blanked_outputs_wake_on_the_first_input_after_resume() {
    let mut h = reach(State::Blanked);
    sleep(&mut h);
    wake(&mut h);
    assert!(h.machine().displays_blanked());
    assert_eq!(h.input(), vec![Command::Unblank { outputs: vec![] }]);
}

#[test]
fn locked_stays_locked_and_restarts_the_delay_on_resume() {
    let mut h = reach(State::Locked);
    h.advance(Duration::from_secs(50));
    assert_eq!(
        sleep(&mut h),
        vec![Command::CancelTimer(TimerId::LockedBlank)]
    );
    assert_eq!(h.state(), State::Locked);
    assert_eq!(
        wake(&mut h),
        vec![Command::SetTimer {
            id: TimerId::LockedBlank,
            after: Duration::from_mins(1),
        }]
    );

    let mut h = Harness::with_config(&config(|c| c.session.when_locked = WhenLocked::Pause));
    h.send(SessionEvent::Locked);
    assert_eq!(sleep(&mut h), vec![]);
    assert_eq!(wake(&mut h), vec![]);
}

#[test]
fn locking_before_sleep_resumes_into_locked() {
    let mut h = reach(State::Blanked);
    h.send(SessionEvent::Locked);
    sleep(&mut h);
    assert_eq!(h.state(), State::Locked);
    assert!(h.timers().is_empty(), "no delay runs while asleep");
    wake(&mut h);
    assert!(h.timers().contains(TimerId::LockedBlank));

    let mut h = Harness::new();
    sleep(&mut h);
    h.send(SessionEvent::Locked);
    assert_eq!(h.state(), State::Active);
    let commands = wake(&mut h);
    assert!(commands.contains(&changed(State::Active, State::Locked)));
}

#[test]
fn unlocking_during_sleep_resumes_into_active() {
    let mut h = reach(State::Locked);
    sleep(&mut h);
    h.send(SessionEvent::Unlocked);
    assert_eq!(h.state(), State::Locked);
    let commands = wake(&mut h);
    assert!(commands.contains(&changed(State::Locked, State::Active)));
    assert!(h.timers().is_empty());
}

#[test]
fn a_pause_outlasts_the_sleep() {
    let during_pause = config(|c| c.safety.ceiling_during_pause = true);
    let mut h = Harness::with_config(&during_pause);
    h.idle();
    h.send(ControlCommand::Pause);
    assert_eq!(sleep(&mut h), vec![Command::CancelTimer(TimerId::Capture)]);
    assert_eq!(h.state(), State::Paused);
    assert_eq!(wake(&mut h), vec![]);
    assert_eq!(h.state(), State::Paused);
    assert!(h.timers().is_empty());
    // Captures pick up again with the next idle period.
    h.idle();
    assert!(h.timers().contains(TimerId::Capture));
}

#[test]
fn stray_timers_while_asleep_are_dropped() {
    let mut h = reach(State::Snoozed);
    sleep(&mut h);
    assert_eq!(h.send(Event::Timer(TimerId::SnoozeExpiry)), vec![]);
    assert_eq!(h.state(), State::Active);
}
