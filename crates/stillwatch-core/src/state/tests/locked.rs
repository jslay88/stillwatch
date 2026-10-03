//! `session.when_locked` in both modes.

use std::time::Duration;

use super::transitions::reach;
use super::{changed, config, transition_record};
use crate::backend::BackendError;
use crate::command::{BlankMethod, Command};
use crate::config::{ActionMode, WhenLocked};
use crate::event::{Event, SessionEvent};
use crate::mocks::Harness;
use crate::state::State;
use crate::time::TimerId;

const MINUTE: Duration = Duration::from_secs(60);

fn blank() -> Command {
    Command::Blank {
        outputs: vec![],
        method: BlankMethod::Dpms,
    }
}

fn paused_when_locked() -> Harness {
    Harness::with_config(&config(|c| c.session.when_locked = WhenLocked::Pause))
}

#[test]
fn blank_after_blanks_once_locked_for_the_delay_without_a_prompt() {
    let mut h = reach(State::Monitoring);
    let commands = h.send(SessionEvent::Locked);
    assert_eq!(
        commands.last(),
        Some(&Command::SetTimer {
            id: TimerId::LockedBlank,
            after: MINUTE,
        })
    );
    h.advance(Duration::from_secs(59));
    assert_eq!(h.state(), State::Locked);

    let commands = h.advance(Duration::from_secs(1));
    assert!(commands.contains(&changed(State::Locked, State::Acting)));
    assert!(commands.contains(&blank()));
    assert!(!commands.iter().any(|c| matches!(
        c,
        Command::ShowPrompt(_) | Command::RequestCapture { .. } | Command::Lock
    )));
    let commands = h.send(Event::ActionCompleted);
    let entry = transition_record(&commands);
    assert_eq!(entry.to, Some(State::Blanked));
    assert!(entry.context.locked);
    assert!(h.machine().displays_blanked());
}

#[test]
fn the_delay_follows_locked_blank_seconds() {
    let mut h = Harness::with_config(&config(|c| c.session.locked_blank_seconds = 5));
    h.send(SessionEvent::Locked);
    assert_eq!(
        h.remaining(TimerId::LockedBlank),
        Some(Duration::from_secs(5))
    );
    h.advance(Duration::from_secs(5));
    assert_eq!(h.state(), State::Acting);
}

#[test]
fn lock_and_blank_skips_the_lock_when_already_locked() {
    let mut h = Harness::with_config(&config(|c| c.action.mode = ActionMode::LockAndBlank));
    h.send(SessionEvent::Locked);
    let commands = h.fire(TimerId::LockedBlank);
    assert!(commands.contains(&blank()));
    assert!(!commands.contains(&Command::Lock));
}

#[test]
fn pause_mode_waits_for_unlock() {
    let mut h = paused_when_locked();
    h.idle();
    let commands = h.send(SessionEvent::Locked);
    assert!(
        !commands
            .iter()
            .any(|c| matches!(c, Command::SetTimer { .. }))
    );
    assert!(h.timers().is_empty());
    assert_eq!(h.advance(60 * MINUTE), vec![]);
    assert_eq!(h.input(), vec![]);
    assert_eq!(h.state(), State::Locked);
    h.send(SessionEvent::Unlocked);
    assert_eq!(h.state(), State::Active);
}

#[test]
fn unlock_returns_to_active_and_cancels_the_delay() {
    let mut h = reach(State::Locked);
    let commands = h.send(SessionEvent::Unlocked);
    assert_eq!(
        commands[..2],
        [
            Command::CancelTimer(TimerId::LockedBlank),
            changed(State::Locked, State::Active)
        ]
    );
    assert!(h.timers().is_empty());
    assert!(!transition_record(&commands).context.locked);
}

#[test]
fn input_while_locked_and_blanked_wakes_then_blanks_again() {
    let mut h = reach(State::Locked);
    h.fire(TimerId::LockedBlank);
    h.send(Event::ActionCompleted);
    assert_eq!(h.state(), State::Blanked);

    let commands = h.input();
    assert_eq!(commands[0], Command::Unblank { outputs: vec![] });
    assert!(commands.contains(&changed(State::Blanked, State::Active)));
    assert!(commands.contains(&changed(State::Active, State::Locked)));
    assert_eq!(h.state(), State::Locked);
    assert_eq!(h.remaining(TimerId::LockedBlank), Some(MINUTE));

    let commands = h.advance(MINUTE);
    assert!(commands.contains(&blank()));
    h.send(Event::ActionCompleted);
    assert_eq!(h.state(), State::Blanked);
}

#[test]
fn locking_while_blanked_then_input_lands_in_locked() {
    let mut h = reach(State::Blanked);
    h.send(SessionEvent::Locked);
    assert_eq!(h.state(), State::Blanked);
    h.gamepad();
    assert_eq!(h.state(), State::Locked);
    assert!(h.timers().contains(TimerId::LockedBlank));

    let mut h = paused_when_locked();
    h.to_blanked();
    h.send(SessionEvent::Locked);
    h.input();
    assert_eq!(h.state(), State::Locked);
    assert!(h.timers().is_empty());
}

#[test]
fn input_while_locked_restarts_the_delay() {
    let mut h = reach(State::Locked);
    h.advance(Duration::from_secs(45));
    h.input();
    h.advance(Duration::from_secs(45));
    assert_eq!(h.state(), State::Locked);
    h.advance(Duration::from_secs(15));
    assert_eq!(h.state(), State::Acting);
}

#[test]
fn input_or_failure_during_a_locked_blank_goes_back_to_locked() {
    let mut h = reach(State::Locked);
    h.fire(TimerId::LockedBlank);
    h.input();
    assert_eq!(h.state(), State::Locked);
    assert_eq!(h.remaining(TimerId::LockedBlank), Some(MINUTE));

    h.idle();
    h.fire(TimerId::LockedBlank);
    let failed = Event::ActionFailed {
        error: BackendError::Io("kscreen-doctor exited 1".into()),
    };
    let commands = h.send(failed);
    assert!(commands.contains(&changed(State::Acting, State::Active)));
    assert_eq!(h.state(), State::Locked);
    assert!(h.timers().contains(TimerId::LockedBlank));
}

#[test]
fn reload_switches_between_modes_while_locked() {
    let mut h = reach(State::Locked);
    let pause = config(|c| c.session.when_locked = WhenLocked::Pause);
    let commands = h.apply_config(&pause);
    assert_eq!(commands[0], Command::CancelTimer(TimerId::LockedBlank));
    assert!(h.timers().is_empty());

    let commands = h.apply_config(&config(|_| {}));
    assert_eq!(
        commands[0],
        Command::SetTimer {
            id: TimerId::LockedBlank,
            after: MINUTE,
        }
    );
    h.advance(Duration::from_secs(30));
    let commands = h.apply_config(&config(|_| {}));
    assert_eq!(commands.len(), 1, "an armed delay keeps its deadline");
    assert_eq!(
        h.remaining(TimerId::LockedBlank),
        Some(Duration::from_secs(30))
    );
}

#[test]
fn resuming_while_locked_hands_over_to_locked() {
    let mut h = paused_when_locked();
    h.send(crate::event::ControlCommand::Pause);
    h.send(SessionEvent::Locked);
    h.send(crate::event::ControlCommand::Resume);
    assert_eq!(
        h.transitions()[1..],
        [
            (State::Paused, State::Active),
            (State::Active, State::Locked)
        ]
    );
    assert!(h.timers().is_empty());
}
