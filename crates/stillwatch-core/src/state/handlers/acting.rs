//! Acting: the configured `action.mode` runs one step at a time, each step
//! confirmed by `Event::ActionCompleted`.

use super::super::context::ActionStep;
use super::{Ctx, State, StateHandler, Transition, is_input};
use crate::command::{Command, HookKind};
use crate::config::ActionMode;
use crate::event::Event;
use crate::history::HistoryKind;

pub(super) struct Handler;

impl StateHandler for Handler {
    fn enter(&self, ctx: &mut Ctx, _via: &Transition) -> Option<Transition> {
        let steps: &[ActionStep] = match ctx.config.action.mode {
            // The dim phase needs the overlay backend; until then it blanks directly.
            ActionMode::Blank | ActionMode::DimThenBlank => &[ActionStep::Blank],
            ActionMode::LockAndBlank => &[ActionStep::Lock, ActionStep::Blank],
            ActionMode::Command => {
                // Hooks are fire and forget, so there is nothing to wait for.
                ctx.hook(HookKind::ActionCommand);
                return Some(Transition::to(State::Blanked));
            }
        };
        ctx.action_steps = steps.iter().copied().collect();
        start_next(ctx);
        None
    }

    fn exit(&self, ctx: &mut Ctx) {
        ctx.action_steps.clear();
    }

    fn on_event(&self, ctx: &mut Ctx, event: &Event) -> Option<Transition> {
        match event {
            Event::ActionCompleted => {
                if start_next(ctx) {
                    None
                } else {
                    let method = ctx.config.action.blank_method;
                    Some(Transition::to(State::Blanked).with_blank_method(method))
                }
            }
            Event::ActionFailed { .. } => Some(Transition::to(ctx.watch_or_active())),
            event if is_input(event) => Some(Transition::to(State::Active)),
            _ => None,
        }
    }
}

/// Starts the next queued step. Returns `false` when none are left.
fn start_next(ctx: &mut Ctx) -> bool {
    let Some(step) = ctx.action_steps.pop_front() else {
        return false;
    };
    match step {
        ActionStep::Lock => ctx.emit(Command::Lock),
        ActionStep::Blank => {
            let outputs = ctx.action_outputs();
            let method = ctx.config.action.blank_method;
            ctx.blanked = Some(outputs.clone());
            ctx.emit(Command::Blank { outputs, method });
            let entry = ctx.history(HistoryKind::Blank).with_blank_method(method);
            ctx.emit(Command::Record(entry));
        }
    }
    true
}
