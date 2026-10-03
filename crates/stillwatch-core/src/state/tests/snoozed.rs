//! Snooze expiry and the snooze ceiling (Snoozed, and Paused with
//! `ceiling_during_pause`).

use std::time::Duration;

use super::transitions::reach;
use super::{changed, config, records_of, transition_record};
use crate::command::Command;
use crate::event::{ControlCommand, Event, SessionEvent};
use crate::history::HistoryKind;
use crate::mocks::Harness;
use crate::state::State;
use crate::stats::ThresholdReason;
use crate::time::TimerId;

const MINUTE: Duration = Duration::from_secs(60);

fn capture_requested(commands: &[Command]) -> bool {
    commands
        .iter()
        .any(|command| matches!(command, Command::RequestCapture { .. }))
}

#[test]
fn expiry_while_idle_watches_again_with_fresh_counters() {
    let mut h = reach(State::Snoozed);
    let resets = h.detector().resets();
    let commands = h.advance(15 * MINUTE);
    assert!(commands.contains(&changed(State::Snoozed, State::Monitoring)));
    assert_eq!(h.state(), State::Monitoring);
    assert_eq!(h.detector().resets(), resets + 1);
    assert!(capture_requested(&commands));
    assert_eq!(h.remaining(TimerId::Capture), Some(MINUTE));
    assert_eq!(h.status().snooze_remaining, None);
}

#[test]
fn expiry_while_active_goes_to_active_without_capturing() {
    let mut h = Harness::new();
    h.send(ControlCommand::Snooze(15 * MINUTE));
    let commands = h.advance(15 * MINUTE);
    assert!(commands.contains(&changed(State::Snoozed, State::Active)));
    assert_eq!(h.state(), State::Active);
    assert_eq!(h.detector().resets(), 0);
    assert!(!capture_requested(&commands));
    assert!(h.timers().is_empty());
}

#[test]
fn expiry_while_locked_hands_over_to_locked() {
    let mut h = reach(State::Snoozed);
    h.send(SessionEvent::Locked);
    h.advance(15 * MINUTE);
    assert_eq!(
        h.transitions()[3..],
        [
            (State::Snoozed, State::Active),
            (State::Active, State::Locked)
        ]
    );
    assert_eq!(h.remaining(TimerId::LockedBlank), Some(MINUTE));
}

#[test]
fn ceiling_during_snooze_triggers_prompting() {
    let mut h = reach(State::Snoozed);
    assert_eq!(h.remaining(TimerId::Capture), Some(MINUTE));
    let resets = h.detector().resets();

    let commands = h.advance(MINUTE);
    assert!(capture_requested(&commands));
    assert_eq!(h.remaining(TimerId::Capture), Some(MINUTE));
    // A stale verdict on the normal threshold doesn't end a snooze.
    h.detector().push_verdict(true);
    assert_eq!(h.capture_ceiling(false), vec![]);
    assert_eq!(h.state(), State::Snoozed);

    let commands = h.capture_ceiling(true);
    assert_eq!(h.state(), State::Prompting);
    let ceiling = records_of(&commands, HistoryKind::Ceiling);
    assert_eq!(ceiling.len(), 1);
    let reason = |entry: &crate::history::HistoryEntry| {
        entry.detection.as_ref().map(|stats| stats.threshold.reason)
    };
    assert_eq!(reason(&ceiling[0]), Some(ThresholdReason::Ceiling));
    assert_eq!(
        reason(&transition_record(&commands)),
        Some(ThresholdReason::Ceiling)
    );
    assert!(commands.contains(&Command::CancelTimer(TimerId::SnoozeExpiry)));
    assert!(commands.contains(&Command::CancelTimer(TimerId::Capture)));
    assert!(
        commands
            .iter()
            .any(|command| matches!(command, Command::ShowPrompt(_)))
    );
    // The counters carried on from Monitoring.
    assert_eq!(h.detector().resets(), resets);
    assert_eq!(h.detector().observations().len(), 3);
}

#[test]
fn ceiling_captures_only_while_idle() {
    let mut h = Harness::new();
    h.send(ControlCommand::Snooze(60 * MINUTE));
    assert!(!h.timers().contains(TimerId::Capture));

    let commands = h.idle();
    assert_eq!(h.detector().resets(), 1);
    assert!(capture_requested(&commands));
    assert_eq!(h.remaining(TimerId::Capture), Some(MINUTE));
    // A repeated idle report doesn't restart the watch.
    assert_eq!(h.idle(), vec![]);

    assert_eq!(h.input(), vec![Command::CancelTimer(TimerId::Capture)]);
    // A capture that lands after the user came back is ignored.
    assert_eq!(h.capture_ceiling(true), vec![]);
    assert_eq!(h.detector().ceiling_queries(), 0);
    assert_eq!(h.state(), State::Snoozed);

    h.idle();
    assert_eq!(h.detector().resets(), 2);
}

#[test]
fn ceiling_disabled_never_captures() {
    let mut h = Harness::with_config(&config(|c| c.safety.ceiling_enabled = false));
    h.to_prompting();
    h.answer(crate::prompt::PromptOutcome::Snooze(60 * MINUTE));
    assert!(!h.timers().contains(TimerId::Capture));
    let commands = h.advance(59 * MINUTE);
    assert!(!capture_requested(&commands));
    assert_eq!(h.capture_ceiling(true), vec![]);
    assert_eq!(h.detector().ceiling_queries(), 0);
    assert_eq!(h.state(), State::Snoozed);
}

#[test]
fn a_detector_without_a_ceiling_never_prompts() {
    let mut h = reach(State::Snoozed);
    h.detector().set_ceiling(None);
    assert_eq!(h.complete_capture(), vec![]);
    assert_eq!(h.detector().ceiling_queries(), 1);
    assert_eq!(h.state(), State::Snoozed);
}

#[test]
fn reload_rearms_starts_or_stops_the_ceiling() {
    let mut h = reach(State::Snoozed);
    let faster = config(|c| c.stale.check_interval_seconds = 30);
    h.apply_config(&faster);
    assert_eq!(h.remaining(TimerId::Capture), Some(Duration::from_secs(30)));

    let off = config(|c| c.safety.ceiling_enabled = false);
    let commands = h.apply_config(&off);
    assert_eq!(commands[0], Command::CancelTimer(TimerId::Capture));
    assert!(!h.timers().contains(TimerId::Capture));

    let resets = h.detector().resets();
    let commands = h.apply_config(&faster);
    assert!(capture_requested(&commands));
    assert_eq!(h.detector().resets(), resets + 1);
}

#[test]
fn ceiling_during_pause_only_when_enabled() {
    let mut h = reach(State::Monitoring);
    h.send(ControlCommand::Pause);
    assert!(!h.timers().contains(TimerId::Capture));
    assert_eq!(h.capture_ceiling(true), vec![]);
    assert_eq!(h.state(), State::Paused);

    let during_pause = config(|c| c.safety.ceiling_during_pause = true);
    let mut h = Harness::with_config(&during_pause);
    h.idle();
    h.send(ControlCommand::Pause);
    assert_eq!(h.remaining(TimerId::Capture), Some(MINUTE));
    assert!(capture_requested(&h.advance(MINUTE)));
    assert_eq!(h.capture_ceiling(false), vec![]);
    let commands = h.capture_ceiling(true);
    assert!(commands.contains(&changed(State::Paused, State::Prompting)));
    assert_eq!(records_of(&commands, HistoryKind::Ceiling).len(), 1);
}

#[test]
fn ceiling_during_pause_follows_reloads() {
    let mut h = reach(State::Monitoring);
    h.send(ControlCommand::Pause);
    let commands = h.apply_config(&config(|c| c.safety.ceiling_during_pause = true));
    assert!(capture_requested(&commands));
    let commands = h.apply_config(&config(|_| {}));
    assert_eq!(commands[0], Command::CancelTimer(TimerId::Capture));
    assert_eq!(
        h.send(Event::Timer(TimerId::Capture)),
        vec![],
        "a late tick doesn't restart captures"
    );
}
