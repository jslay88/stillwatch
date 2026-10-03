//! One test per transition.

use std::time::Duration;

use super::{changed, effects, records, transition_record};
use crate::command::{BlankMethod, Command};
use crate::event::{ControlCommand, Event, SessionEvent};
use crate::history::{HistoryKind, PromptAnswer};
use crate::mocks::Harness;
use crate::prompt::{PromptOutcome, PromptRequest, StaleOutput};
use crate::state::State;
use crate::time::TimerId;

const MINUTE: Duration = Duration::from_secs(60);

#[test]
fn active_to_monitoring_on_idle_starts_capturing() {
    let mut h = Harness::new();
    let commands = h.idle();
    assert_eq!(commands[0], changed(State::Active, State::Monitoring));
    assert!(matches!(commands[1], Command::Record(_)));
    let entry = transition_record(&commands);
    assert_eq!(
        (entry.from, entry.to),
        (Some(State::Active), Some(State::Monitoring))
    );
    assert_eq!(
        commands[2..],
        [
            Command::RequestCapture {
                outputs: vec![],
                downscale_width: 480,
            },
            Command::SetTimer {
                id: TimerId::Capture,
                after: MINUTE,
            },
        ]
    );
    assert_eq!(h.detector().resets(), 1);
    assert_eq!(h.state(), State::Monitoring);
}

#[test]
fn monitoring_captures_every_check_interval() {
    let mut h = Harness::new();
    h.idle();
    let commands = h.advance(3 * MINUTE);
    let captures = commands
        .iter()
        .filter(|command| matches!(command, Command::RequestCapture { .. }))
        .count();
    assert_eq!(captures, 3);
    assert_eq!(h.remaining(TimerId::Capture), Some(MINUTE));
    assert_eq!(h.state(), State::Monitoring);
}

#[test]
fn monitoring_stays_on_a_fresh_capture() {
    let mut h = Harness::new();
    h.idle();
    assert_eq!(h.capture(false), vec![]);
    assert_eq!(h.state(), State::Monitoring);
    assert_eq!(h.detector().observations().len(), 1);
    assert!(h.status().last_detection.is_some());
}

#[test]
fn monitoring_to_active_on_input_stops_capturing() {
    let mut h = Harness::new();
    h.idle();
    let commands = h.input();
    assert_eq!(
        commands[..2],
        [
            Command::CancelTimer(TimerId::Capture),
            changed(State::Monitoring, State::Active)
        ]
    );
    assert!(h.timers().is_empty());
    assert_eq!(h.advance(10 * MINUTE), vec![]);
}

#[test]
fn monitoring_to_prompting_on_stale_shows_the_prompt() {
    let mut h = Harness::new();
    h.idle();
    let commands = h.capture(true);
    assert_eq!(commands[0], Command::CancelTimer(TimerId::Capture));
    assert_eq!(commands[1], changed(State::Monitoring, State::Prompting));
    let entry = transition_record(&commands);
    assert!(entry.detection.is_some_and(|stats| stats.stale));
    assert_eq!(
        commands[3..5],
        [
            Command::ShowPrompt(PromptRequest {
                countdown: MINUTE,
                presets: vec![15 * MINUTE, 60 * MINUTE, 180 * MINUTE],
                allow_custom: true,
                stale_outputs: vec![StaleOutput {
                    output: "HDMI-A-1".into(),
                    unchanged_percent: 100,
                }],
            }),
            Command::SetTimer {
                id: TimerId::PromptCountdown,
                after: MINUTE,
            },
        ]
    );
    assert!(matches!(&commands[5], Command::Record(e) if e.kind == HistoryKind::Prompt));
    assert!(!h.timers().contains(TimerId::Capture));
}

#[test]
fn prompting_to_snoozed_on_snooze() {
    let mut h = Harness::new();
    h.to_prompting();
    let commands = h.answer(PromptOutcome::Snooze(15 * MINUTE));
    let answer = &records(&commands)[0];
    assert_eq!(answer.kind, HistoryKind::PromptAnswered);
    assert_eq!(
        (answer.answer, answer.snooze_seconds),
        (Some(PromptAnswer::Snooze), Some(900))
    );
    assert_eq!(
        commands[1..3],
        [
            Command::CancelTimer(TimerId::PromptCountdown),
            Command::DismissPrompt
        ]
    );
    assert_eq!(commands[3], changed(State::Prompting, State::Snoozed));
    assert_eq!(transition_record(&commands).snooze_seconds, Some(900));
    assert_eq!(h.remaining(TimerId::SnoozeExpiry), Some(15 * MINUTE));
    assert!(!h.timers().contains(TimerId::PromptCountdown));
    assert_eq!(h.status().snooze_remaining, Some(15 * MINUTE));
}

#[test]
fn prompting_to_active_on_cancel() {
    let mut h = Harness::new();
    h.to_prompting();
    let commands = h.answer(PromptOutcome::Cancel);
    assert!(commands.contains(&Command::DismissPrompt));
    assert!(commands.contains(&changed(State::Prompting, State::Active)));
    assert!(h.timers().is_empty());
}

#[test]
fn prompting_to_active_once_the_answer_grace_after_input_passes() {
    let mut h = Harness::new();
    h.to_prompting();
    assert_eq!(
        h.input(),
        vec![Command::SetTimer {
            id: TimerId::PromptAnswerGrace,
            after: Duration::from_secs(10),
        }]
    );
    let commands = h.fire(TimerId::PromptAnswerGrace);
    assert!(commands.contains(&Command::DismissPrompt));
    assert!(commands.contains(&changed(State::Prompting, State::Active)));
    assert!(h.timers().is_empty());
}

#[test]
fn prompting_to_acting_when_the_countdown_runs_out() {
    let mut h = Harness::new();
    h.to_prompting();
    h.advance(Duration::from_secs(59));
    assert_eq!(h.state(), State::Prompting);
    let commands = h.advance(Duration::from_secs(1));
    assert!(commands.contains(&changed(State::Prompting, State::Acting)));
    assert!(commands.contains(&Command::Blank {
        outputs: vec![],
        method: BlankMethod::Dpms,
    }));
}

#[test]
fn prompting_to_acting_on_prompter_timeout() {
    let mut h = Harness::new();
    h.to_prompting();
    let commands = h.answer(PromptOutcome::Timeout);
    assert!(commands.contains(&changed(State::Prompting, State::Acting)));
    assert!(!h.timers().contains(TimerId::PromptCountdown));
}

#[test]
fn unanswered_prompts_still_time_out() {
    let mut h = Harness::new();
    h.to_prompting();
    let failed = Event::PromptFailed {
        error: crate::backend::BackendError::Unavailable("no server".into()),
    };
    let mut answers = Vec::new();
    for commands in [h.answer(PromptOutcome::Dismissed), h.send(failed)] {
        assert_eq!(effects(&commands), vec![]);
        let entry = &records(&commands)[0];
        assert_eq!(entry.kind, HistoryKind::PromptAnswered);
        answers.push(entry.answer);
    }
    assert_eq!(
        answers,
        [Some(PromptAnswer::Dismissed), Some(PromptAnswer::Failed)]
    );
    assert_eq!(h.state(), State::Prompting);
    let commands = h.fire(TimerId::PromptCountdown);
    assert_eq!(records(&commands)[0].answer, Some(PromptAnswer::Timeout));
    assert_eq!(h.state(), State::Acting);
}

#[test]
fn acting_to_blanked_on_action_completed() {
    let mut h = Harness::new();
    h.to_prompting();
    h.fire(TimerId::PromptCountdown);
    let commands = h.send(Event::ActionCompleted);
    assert_eq!(commands[0], changed(State::Acting, State::Blanked));
    let entry = transition_record(&commands);
    assert_eq!(entry.blank_method, Some(BlankMethod::Dpms));
    assert_eq!(commands.len(), 2, "no hooks are configured: {commands:?}");
    assert!(h.machine().displays_blanked());
}

#[test]
fn acting_to_active_on_input_wakes_the_displays() {
    let mut h = Harness::new();
    h.to_prompting();
    h.fire(TimerId::PromptCountdown);
    let commands = h.input();
    assert_eq!(commands[0], Command::Unblank { outputs: vec![] });
    assert!(commands.contains(&changed(State::Acting, State::Active)));
    assert!(!h.machine().displays_blanked());
}

#[test]
fn acting_to_monitoring_when_the_action_fails_while_idle() {
    let mut h = Harness::new();
    h.to_prompting();
    h.fire(TimerId::PromptCountdown);
    let failed = Event::ActionFailed {
        error: crate::backend::BackendError::Io("kscreen-doctor exited 1".into()),
    };
    let commands = h.send(failed);
    assert!(commands.contains(&changed(State::Acting, State::Monitoring)));
    assert_eq!(h.detector().resets(), 2);
    assert!(h.timers().contains(TimerId::Capture));
}

#[test]
fn blanked_to_active_on_input() {
    let mut h = Harness::new();
    h.to_blanked();
    let commands = h.input();
    assert_eq!(
        commands[..2],
        [
            Command::Unblank { outputs: vec![] },
            changed(State::Blanked, State::Active)
        ]
    );
    assert!(!h.machine().displays_blanked());
}

#[test]
fn snooze_command_from_active_and_monitoring() {
    let mut h = Harness::new();
    h.send(ControlCommand::Snooze(60 * MINUTE));
    assert_eq!(h.state(), State::Snoozed);

    let mut h = Harness::new();
    h.idle();
    let commands = h.send(ControlCommand::Snooze(45 * MINUTE));
    assert!(commands.contains(&Command::CancelTimer(TimerId::Capture)));
    assert!(commands.contains(&changed(State::Monitoring, State::Snoozed)));
    assert_eq!(h.remaining(TimerId::SnoozeExpiry), Some(45 * MINUTE));
}

#[test]
fn cancel_snooze_goes_to_monitoring_while_idle() {
    let mut h = Harness::new();
    h.to_prompting();
    h.answer(PromptOutcome::Snooze(15 * MINUTE));
    let commands = h.send(ControlCommand::CancelSnooze);
    assert_eq!(
        commands[..3],
        [
            Command::CancelTimer(TimerId::SnoozeExpiry),
            Command::CancelTimer(TimerId::Capture),
            changed(State::Snoozed, State::Monitoring)
        ]
    );
    assert_eq!(h.detector().resets(), 2);
    assert_eq!(h.status().snooze_remaining, None);
}

#[test]
fn cancel_snooze_goes_to_active_while_present() {
    let mut h = Harness::new();
    h.send(ControlCommand::Snooze(15 * MINUTE));
    h.send(ControlCommand::CancelSnooze);
    assert_eq!(
        h.transitions().last(),
        Some(&(State::Snoozed, State::Active))
    );
}

#[test]
fn snooze_expiry_follows_the_cancel_rule() {
    let mut h = Harness::new();
    h.to_prompting();
    h.answer(PromptOutcome::Snooze(15 * MINUTE));
    h.advance(15 * MINUTE);
    assert_eq!(h.state(), State::Monitoring);
}

#[test]
fn input_cancels_a_snooze_only_when_configured() {
    let mut h = Harness::new();
    h.send(ControlCommand::Snooze(15 * MINUTE));
    assert_eq!(h.input(), vec![]);
    assert_eq!(h.state(), State::Snoozed);

    let mut config = crate::config::Config::default();
    config.prompt.snooze_cancelled_by_input = true;
    let mut h = Harness::with_config(&config);
    h.send(ControlCommand::Snooze(15 * MINUTE));
    h.input();
    assert_eq!(h.state(), State::Active);
}

#[test]
fn session_lock_from_active_and_monitoring_and_unlock() {
    let mut h = Harness::new();
    h.send(SessionEvent::Locked);
    assert_eq!(h.state(), State::Locked);
    h.send(SessionEvent::Unlocked);
    assert_eq!(h.state(), State::Active);

    let mut h = Harness::new();
    h.idle();
    let commands = h.send(SessionEvent::Locked);
    assert!(commands.contains(&Command::CancelTimer(TimerId::Capture)));
    assert_eq!(h.state(), State::Locked);
    assert!(transition_record(&commands).context.locked);
}

#[test]
fn pause_from_every_state_and_resume() {
    for state in State::ALL.into_iter().filter(|s| *s != State::Paused) {
        let mut h = reach(state);
        let commands = h.send(ControlCommand::Pause);
        assert!(commands.contains(&changed(state, State::Paused)), "{state}");
        assert!(h.timers().is_empty(), "{state}: {:?}", h.timers());
        assert_eq!(h.send(ControlCommand::Pause), vec![]);
    }
    let mut h = reach(State::Monitoring);
    h.send(ControlCommand::Pause);
    h.send(ControlCommand::Resume);
    assert_eq!(h.state(), State::Monitoring);

    let mut h = Harness::new();
    h.send(ControlCommand::Pause);
    let commands = h.send(ControlCommand::Resume);
    assert_eq!(commands[0], changed(State::Paused, State::Active));
    assert_eq!(h.send(ControlCommand::Resume), vec![]);
}

#[test]
fn resume_and_cancel_snooze_are_ignored_elsewhere() {
    let mut h = reach(State::Prompting);
    assert_eq!(h.send(ControlCommand::Resume), vec![]);
    assert_eq!(h.send(ControlCommand::CancelSnooze), vec![]);
    assert_eq!(h.send(ControlCommand::Reload), vec![]);
    assert_eq!(h.state(), State::Prompting);
}

#[test]
fn snoozing_again_while_snoozed_rearms_the_timer() {
    let mut h = Harness::new();
    h.send(ControlCommand::Snooze(15 * MINUTE));
    h.advance(10 * MINUTE);
    let commands = h.send(ControlCommand::Snooze(60 * MINUTE));
    assert_eq!(h.state(), State::Snoozed);
    assert_eq!(h.remaining(TimerId::SnoozeExpiry), Some(60 * MINUTE));
    let entries = records(&commands);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].kind, HistoryKind::Snooze);
    assert_eq!(entries[0].snooze_seconds, Some(3600));
}

/// A harness driven into `state` through public events.
pub(super) fn reach(state: State) -> Harness {
    let mut h = Harness::new();
    match state {
        State::Active => {}
        State::Monitoring => {
            h.idle();
        }
        State::Prompting => {
            h.to_prompting();
        }
        State::Snoozed => {
            h.to_prompting();
            h.answer(PromptOutcome::Snooze(15 * MINUTE));
        }
        State::Acting => {
            h.to_prompting();
            h.fire(TimerId::PromptCountdown);
        }
        State::Blanked => {
            h.to_blanked();
        }
        State::Locked => {
            h.send(SessionEvent::Locked);
        }
        State::Paused => {
            h.send(ControlCommand::Pause);
        }
    }
    assert_eq!(h.state(), state);
    h
}
