//! Acting: the configured `action.mode` runs one step at a time, each step
//! confirmed by `Event::ActionCompleted`.
//!
//! A re-blank (from Blanked) only blanks, with the method the watchdog
//! picked. With the session already locked, `lock_and_blank` skips the lock.

use std::collections::VecDeque;

use super::super::context::ActionStep;
use super::{Ctx, State, StateHandler, Transition, is_input};
use crate::command::{Command, HookKind};
use crate::config::ActionMode;
use crate::event::Event;
use crate::history::HistoryKind;

pub(super) struct Handler;

impl StateHandler for Handler {
    fn enter(&self, ctx: &mut Ctx, via: &Transition) -> Option<Transition> {
        let configured = ctx.config.action.blank_method;
        let blank = ActionStep::Blank(via.blank_method.unwrap_or(configured));
        let steps = if via.reblank_attempt.is_some() {
            VecDeque::from([blank])
        } else {
            ctx.reblank_attempts = 0;
            match ctx.config.action.mode {
                // The daemon dims for `dim_seconds` (cancelled by `Unblank`)
                // before executing this Blank when mode is dim_then_blank.
                ActionMode::Blank | ActionMode::DimThenBlank => VecDeque::from([blank]),
                ActionMode::LockAndBlank if ctx.locked => VecDeque::from([blank]),
                ActionMode::LockAndBlank => VecDeque::from([ActionStep::Lock, blank]),
                ActionMode::Command => {
                    // Hooks are fire and forget, so there is nothing to wait for.
                    ctx.hook(HookKind::ActionCommand);
                    return Some(Transition::to(State::Blanked));
                }
            }
        };
        ctx.action_steps = steps;
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
                    Some(Transition::to(State::Blanked).with_blank_method(ctx.blank_method))
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
        ActionStep::Blank(method) => {
            let outputs = ctx.action_outputs();
            ctx.blanked = Some(outputs.clone());
            ctx.blank_method = method;
            ctx.emit(Command::Blank { outputs, method });
            // Re-blanks are recorded as `Reblank` by the Blanked handler.
            if ctx.reblank_attempts == 0 {
                let entry = ctx.history(HistoryKind::Blank).with_blank_method(method);
                ctx.emit(Command::Record(entry));
            }
        }
    }
    true
}
