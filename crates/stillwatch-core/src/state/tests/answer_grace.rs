//! Input during a prompt arms the answer grace instead of leaving, so the
//! click that answers the prompt isn't beaten by the input that reaches it.

use std::time::Duration;

use super::{changed, config, record_kinds, records, records_of};
use crate::command::Command;
use crate::event::{ControlCommand, Event, SessionEvent};
use crate::history::{HistoryKind, PromptAnswer};
use crate::mocks::Harness;
use crate::prompt::PromptOutcome;
use crate::state::State;
use crate::time::TimerId;

const GRACE: Duration = Duration::from_secs(10);
const MINUTE: Duration = Duration::from_secs(60);

/// A harness in Prompting, `after` into the countdown, that just saw input.
fn prompted_then_input(after: Duration) -> Harness {
    let mut h = Harness::new();
    h.to_prompting();
    h.advance(after);
    h.input();
    assert_eq!(h.state(), State::Prompting);
    assert_eq!(h.remaining(TimerId::PromptAnswerGrace), Some(GRACE));
    h
}

/// The prompt ended without blanking anything.
fn assert_never_blanked(h: &Harness) {
    assert!(!h.transitions().contains(&(State::Prompting, State::Acting)));
    assert_eq!(records_of(h.log(), HistoryKind::Blank), vec![]);
    assert!(
        !h.log()
            .iter()
            .any(|command| matches!(command, Command::Blank { .. }))
    );
}

#[test]
fn input_then_snooze_ends_snoozed_for_the_picked_duration() {
    let mut h = prompted_then_input(Duration::ZERO);
    h.advance(Duration::from_secs(2));
    let commands = h.answer(PromptOutcome::Snooze(15 * MINUTE));
    assert!(commands.contains(&Command::CancelTimer(TimerId::PromptAnswerGrace)));
    assert!(commands.contains(&Command::DismissPrompt));
    assert!(commands.contains(&changed(State::Prompting, State::Snoozed)));
    assert_eq!(h.status().snooze_remaining, Some(15 * MINUTE));
    assert_eq!(h.remaining(TimerId::SnoozeExpiry), Some(15 * MINUTE));
    assert!(!h.timers().contains(TimerId::PromptAnswerGrace));

    let answer = &records_of(&commands, HistoryKind::PromptAnswered)[0];
    assert_eq!(
        (answer.answer, answer.snooze_seconds),
        (Some(PromptAnswer::Snooze), Some(900))
    );
    assert_never_blanked(&h);
}

#[test]
fn input_then_nothing_goes_active_after_exactly_the_grace() {
    let mut h = prompted_then_input(Duration::ZERO);
    assert_eq!(h.advance(Duration::from_millis(9_999)), vec![]);
    assert_eq!(h.state(), State::Prompting);

    let commands = h.advance(Duration::from_millis(1));
    assert_eq!(
        commands[..3],
        [
            Command::CancelTimer(TimerId::PromptCountdown),
            Command::DismissPrompt,
            changed(State::Prompting, State::Active),
        ]
    );
    assert_eq!(record_kinds(&commands), [HistoryKind::Transition]);
    assert!(h.timers().is_empty());
    assert_eq!(h.advance(10 * MINUTE), vec![]);
    assert_never_blanked(&h);
}

#[test]
fn countdown_running_out_during_the_grace_goes_active_not_acting() {
    let mut h = prompted_then_input(Duration::from_secs(55));
    let commands = h.advance(Duration::from_secs(5));
    assert!(commands.contains(&Command::CancelTimer(TimerId::PromptAnswerGrace)));
    assert!(commands.contains(&Command::DismissPrompt));
    assert!(commands.contains(&changed(State::Prompting, State::Active)));
    assert_eq!(
        record_kinds(&commands),
        [HistoryKind::PromptAnswered, HistoryKind::Transition]
    );
    let entries = records(&commands);
    assert_eq!(entries[0].answer, Some(PromptAnswer::Timeout));
    assert_eq!(entries[1].to, Some(State::Active));
    assert!(h.timers().is_empty());
    assert_never_blanked(&h);
}

#[test]
fn input_then_cancel_goes_active() {
    let mut h = prompted_then_input(Duration::ZERO);
    let commands = h.answer(PromptOutcome::Cancel);
    assert!(commands.contains(&Command::CancelTimer(TimerId::PromptAnswerGrace)));
    assert!(commands.contains(&changed(State::Prompting, State::Active)));
    assert_eq!(
        records(&commands)[0].answer,
        Some(PromptAnswer::Cancel),
        "{commands:?}"
    );
    assert!(h.timers().is_empty());
    assert_never_blanked(&h);
}

#[test]
fn dismissed_or_prompter_timeout_after_input_goes_active() {
    for outcome in [PromptOutcome::Dismissed, PromptOutcome::Timeout] {
        let mut h = prompted_then_input(Duration::ZERO);
        let commands = h.answer(outcome);
        assert!(commands.contains(&Command::DismissPrompt), "{outcome:?}");
        assert!(commands.contains(&changed(State::Prompting, State::Active)));
        assert_eq!(
            records(&commands)[0].answer,
            Some(PromptAnswer::from(outcome))
        );
        assert!(h.timers().is_empty());
        assert_never_blanked(&h);
    }
}

#[test]
fn more_input_doesnt_extend_the_grace() {
    let mut h = prompted_then_input(Duration::ZERO);
    h.advance(Duration::from_secs(4));
    assert_eq!(h.input(), vec![]);
    assert_eq!(h.gamepad(), vec![]);
    h.advance(Duration::from_secs(5));
    assert_eq!(h.input(), vec![]);
    assert_eq!(
        h.remaining(TimerId::PromptAnswerGrace),
        Some(Duration::from_secs(1))
    );
    h.advance(Duration::from_secs(1));
    assert_eq!(h.state(), State::Active);
}

#[test]
fn custom_restarts_the_grace_for_the_dialog() {
    let mut h = prompted_then_input(Duration::ZERO);
    h.advance(Duration::from_secs(8));
    let commands = h.answer(PromptOutcome::CustomRequested);
    assert_eq!(
        commands[1..],
        [Command::SetTimer {
            id: TimerId::PromptAnswerGrace,
            after: GRACE,
        }]
    );
    assert_eq!(records(&commands)[0].answer, Some(PromptAnswer::Custom));
    h.advance(Duration::from_secs(9));
    assert_eq!(h.state(), State::Prompting);
    h.answer(PromptOutcome::Snooze(60 * MINUTE));
    assert_eq!(h.state(), State::Snoozed);
    assert_eq!(h.status().snooze_remaining, Some(60 * MINUTE));
}

#[test]
fn custom_counts_as_presence_even_before_the_input_arrives() {
    let mut h = Harness::new();
    h.to_prompting();
    h.advance(Duration::from_secs(55));
    h.answer(PromptOutcome::CustomRequested);
    assert_eq!(h.input(), vec![]);
    h.advance(Duration::from_secs(5));
    assert_eq!(h.state(), State::Active);
    assert_never_blanked(&h);
}

#[test]
fn invalid_snooze_or_failure_after_input_waits_out_the_grace() {
    let mut h = prompted_then_input(Duration::ZERO);
    h.answer(PromptOutcome::Snooze(Duration::from_secs(5)));
    let failed = Event::PromptFailed {
        error: crate::backend::BackendError::Unavailable("no server".into()),
    };
    h.send(failed);
    assert_eq!(h.state(), State::Prompting);
    h.advance(GRACE);
    assert_eq!(h.state(), State::Active);
    let answers: Vec<_> = records_of(h.log(), HistoryKind::PromptAnswered)
        .into_iter()
        .map(|entry| entry.answer)
        .collect();
    assert_eq!(
        answers,
        [Some(PromptAnswer::Snooze), Some(PromptAnswer::Failed)]
    );
}

#[test]
fn snooze_command_after_input_still_snoozes() {
    let mut h = prompted_then_input(Duration::ZERO);
    h.send(ControlCommand::Snooze(45 * MINUTE));
    assert_eq!(h.state(), State::Snoozed);
    assert!(!h.timers().contains(TimerId::PromptAnswerGrace));
}

#[test]
fn leaving_by_pause_or_sleep_disarms_the_grace() {
    for leave in [
        Event::Control(ControlCommand::Pause),
        Event::Session(SessionEvent::PrepareForSleep),
    ] {
        let mut h = prompted_then_input(Duration::ZERO);
        let commands = h.send(leave.clone());
        assert!(
            commands.contains(&Command::CancelTimer(TimerId::PromptAnswerGrace)),
            "{leave:?}"
        );
        assert!(h.timers().is_empty(), "{leave:?}: {:?}", h.timers());
    }
}

#[test]
fn a_configured_grace_is_used() {
    let mut h = Harness::with_config(&config(|c| c.prompt.answer_grace_seconds = 30));
    h.to_prompting();
    h.input();
    h.advance(Duration::from_secs(29));
    assert_eq!(h.state(), State::Prompting);
    h.advance(Duration::from_secs(1));
    assert_eq!(h.state(), State::Active);
}

#[test]
fn reload_shortens_an_armed_grace_but_never_extends_it() {
    let mut h = prompted_then_input(Duration::ZERO);
    h.advance(Duration::from_secs(2));
    let longer = config(|c| c.prompt.answer_grace_seconds = 60);
    assert_eq!(
        record_kinds(&h.apply_config(&longer)),
        [HistoryKind::ConfigReload]
    );
    assert_eq!(
        h.remaining(TimerId::PromptAnswerGrace),
        Some(Duration::from_secs(8))
    );

    let shorter = config(|c| c.prompt.answer_grace_seconds = 3);
    let commands = h.apply_config(&shorter);
    assert_eq!(
        commands[0],
        Command::SetTimer {
            id: TimerId::PromptAnswerGrace,
            after: Duration::from_secs(3),
        }
    );
    h.advance(Duration::from_secs(3));
    assert_eq!(h.state(), State::Active);
}

#[test]
fn reload_shorter_than_whats_left_only_when_it_ends_sooner() {
    let mut h = prompted_then_input(Duration::ZERO);
    h.advance(Duration::from_secs(6));
    let shorter = config(|c| c.prompt.answer_grace_seconds = 5);
    h.apply_config(&shorter);
    assert_eq!(
        h.remaining(TimerId::PromptAnswerGrace),
        Some(Duration::from_secs(4))
    );
}

#[test]
fn reload_without_input_arms_nothing() {
    let mut h = Harness::new();
    h.to_prompting();
    let commands = h.apply_config(&config(|c| c.prompt.answer_grace_seconds = 1));
    assert_eq!(record_kinds(&commands), [HistoryKind::ConfigReload]);
    assert_eq!(commands.len(), 1);
    assert!(!h.timers().contains(TimerId::PromptAnswerGrace));
}
