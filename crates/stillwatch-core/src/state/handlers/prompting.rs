//! Prompting: the screen is stale. The prompt is up and the countdown runs.

use super::{Ctx, State, StateHandler, Transition, is_input};
use crate::command::Command;
use crate::event::{ControlCommand, Event};
use crate::history::{HistoryKind, PromptAnswer};
use crate::prompt::PromptOutcome;
use crate::time::TimerId;

pub(super) struct Handler;

impl StateHandler for Handler {
    fn enter(&self, ctx: &mut Ctx, via: &Transition) -> Option<Transition> {
        let request = ctx.prompt_request();
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
                Some(Transition::to(State::Acting))
            }
            event if is_input(event) => Some(Transition::to(State::Active)),
            _ => None,
        }
    }
}

/// Where an answer leads. A dismissed prompt, one waiting on "Custom...", or
/// an invalid snooze keeps Prompting, so the countdown still acts.
fn answered(ctx: &Ctx, outcome: PromptOutcome) -> Option<Transition> {
    match outcome {
        PromptOutcome::Snooze(duration) => ctx.snooze(duration),
        PromptOutcome::Cancel => Some(Transition::to(State::Active)),
        PromptOutcome::Timeout => Some(Transition::to(State::Acting)),
        PromptOutcome::CustomRequested | PromptOutcome::Dismissed => None,
    }
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
