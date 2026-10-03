//! Prompting: the screen is stale. The prompt is up and the countdown runs.
//!
//! Input doesn't end the prompt right away. Reaching a notification action
//! takes a mouse move or key press, and the compositor reports that input
//! before the prompter reports the click. So the first input arms the answer
//! grace (`prompt.answer_grace_seconds`); more input doesn't push it back.
//! From then on the user counts as present: answers work as usual, and the
//! grace or our countdown running out, or a dismissal, go to Active instead
//! of the action. A prompter's `Timeout` still acts, because a prompter only
//! sends it when asked to act now ("Blank now"). Picking "Custom..." also
//! counts as presence and restarts the grace, so the dialog gets the full
//! time from the click (still capped by the countdown).

use std::time::Duration;

use super::{Ctx, State, StateHandler, Transition, is_input};
use crate::command::Command;
use crate::event::{ActivityEvent, ControlCommand, Event};
use crate::history::{HistoryKind, PromptAnswer};
use crate::prompt::PromptOutcome;
use crate::time::TimerId;

pub(super) struct Handler;

impl StateHandler for Handler {
    fn enter(&self, ctx: &mut Ctx, via: &Transition) -> Option<Transition> {
        let request = ctx.prompt_request(via.detection.as_ref());
        let countdown = request.countdown;
        ctx.emit(Command::ShowPrompt(request));
        ctx.set_timer(TimerId::PromptCountdown, countdown);
        let mut entry = ctx.history(HistoryKind::Prompt);
        entry.detection.clone_from(&via.detection);
        ctx.emit(Command::Record(entry));
        None
    }

    fn exit(&self, ctx: &mut Ctx) {
        ctx.cancel_timer(TimerId::PromptCountdown);
        ctx.disarm(TimerId::PromptAnswerGrace);
        ctx.answer_grace_until = None;
        ctx.emit(Command::DismissPrompt);
    }

    fn on_event(&self, ctx: &mut Ctx, event: &Event) -> Option<Transition> {
        match event {
            Event::PromptAnswered(outcome) => {
                record_answer(ctx, *outcome);
                answered(ctx, *outcome)
            }
            Event::PromptFailed { .. } => {
                let entry = ctx
                    .history(HistoryKind::PromptAnswered)
                    .with_answer(PromptAnswer::Failed);
                ctx.emit(Command::Record(entry));
                None
            }
            Event::Control(ControlCommand::Snooze(duration)) => ctx.snooze(*duration),
            Event::Timer(TimerId::PromptCountdown) => {
                record_answer(ctx, PromptOutcome::Timeout);
                Some(ran_out(ctx))
            }
            Event::Timer(TimerId::PromptAnswerGrace) | Event::Activity(ActivityEvent::Unknown) => {
                Some(Transition::to(State::Active))
            }
            event if is_input(event) => {
                if !present(ctx) {
                    start_grace(ctx);
                }
                None
            }
            _ => None,
        }
    }

    /// An armed grace keeps its deadline unless the new length, counted from
    /// now, ends sooner.
    fn reconfigure(&self, ctx: &mut Ctx) {
        let Some(until) = ctx.answer_grace_until else {
            return;
        };
        if ctx
            .now
            .checked_add(grace(ctx))
            .is_some_and(|sooner| sooner < until)
        {
            start_grace(ctx);
        }
    }
}

/// Where an answer leads. A dismissed prompt keeps Prompting until the
/// countdown acts, unless the user is present. One waiting on "Custom..." or
/// an invalid snooze keeps Prompting until the countdown or the grace runs
/// out.
fn answered(ctx: &mut Ctx, outcome: PromptOutcome) -> Option<Transition> {
    match outcome {
        PromptOutcome::Snooze(duration) => ctx.snooze(duration),
        PromptOutcome::Cancel => Some(Transition::to(State::Active)),
        PromptOutcome::Timeout => Some(Transition::to(State::Acting)),
        PromptOutcome::Dismissed => present(ctx).then(|| Transition::to(State::Active)),
        PromptOutcome::CustomRequested => {
            start_grace(ctx);
            None
        }
    }
}

/// Input arrived while the prompt was up (or "Custom..." was picked).
fn present(ctx: &Ctx) -> bool {
    ctx.is_armed(TimerId::PromptAnswerGrace)
}

/// Where the prompt goes when our countdown runs out.
fn ran_out(ctx: &Ctx) -> Transition {
    if present(ctx) || !ctx.activity_known() {
        Transition::to(State::Active)
    } else {
        Transition::to(State::Acting)
    }
}

fn grace(ctx: &Ctx) -> Duration {
    Duration::from_secs(u64::from(ctx.config.prompt.answer_grace_seconds))
}

fn start_grace(ctx: &mut Ctx) {
    let grace = grace(ctx);
    ctx.answer_grace_until = ctx.now.checked_add(grace);
    ctx.set_timer(TimerId::PromptAnswerGrace, grace);
}

fn record_answer(ctx: &mut Ctx, outcome: PromptOutcome) {
    let mut entry = ctx
        .history(HistoryKind::PromptAnswered)
        .with_answer(outcome.into());
    if let PromptOutcome::Snooze(duration) = outcome {
        entry = entry.with_snooze(duration);
    }
    ctx.emit(Command::Record(entry));
}
