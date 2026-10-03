//! Prompting: the screen is stale. The prompt is up and the countdown runs.

use super::{Ctx, State, StateHandler, Transition, is_input};
use crate::command::Command;
use crate::event::{ControlCommand, Event};
use crate::prompt::PromptOutcome;
use crate::time::TimerId;

pub(super) struct Handler;

impl StateHandler for Handler {
    fn enter(&self, ctx: &mut Ctx, _via: &Transition) -> Option<Transition> {
        let request = ctx.prompt_request();
        let countdown = request.countdown;
        ctx.emit(Command::ShowPrompt(request));
        ctx.set_timer(TimerId::PromptCountdown, countdown);
        None
    }

    fn exit(&self, ctx: &mut Ctx) {
        ctx.cancel_timer(TimerId::PromptCountdown);
        ctx.emit(Command::DismissPrompt);
    }

    fn on_event(&self, ctx: &mut Ctx, event: &Event) -> Option<Transition> {
        match event {
            Event::PromptAnswered(PromptOutcome::Snooze(duration))
            | Event::Control(ControlCommand::Snooze(duration)) => ctx.snooze(*duration),
            Event::PromptAnswered(PromptOutcome::Cancel) => Some(Transition::to(State::Active)),
            Event::PromptAnswered(PromptOutcome::Timeout)
            | Event::Timer(TimerId::PromptCountdown) => Some(Transition::to(State::Acting)),
            event if is_input(event) => Some(Transition::to(State::Active)),
            // A dismissed or failed prompt, one waiting on "Custom...", or an
            // invalid snooze still acts when the countdown runs out.
            _ => None,
        }
    }
}
