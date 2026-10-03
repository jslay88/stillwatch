//! Snoozed: prompting waits until the snooze ends.
//!
//! Expiry follows the same rule as cancelling. The snooze ceiling isn't
//! handled yet.

use std::time::Duration;

use super::{Ctx, State, StateHandler, Transition, is_input};
use crate::command::Command;
use crate::event::{ControlCommand, Event};
use crate::history::HistoryKind;
use crate::time::TimerId;

pub(super) struct Handler;

impl StateHandler for Handler {
    fn enter(&self, ctx: &mut Ctx, via: &Transition) -> Option<Transition> {
        if let Some(duration) = via.snooze {
            start(ctx, duration);
        }
        None
    }

    fn exit(&self, ctx: &mut Ctx) {
        ctx.cancel_timer(TimerId::SnoozeExpiry);
        ctx.snooze_until = None;
    }

    fn on_event(&self, ctx: &mut Ctx, event: &Event) -> Option<Transition> {
        match event {
            Event::Control(ControlCommand::CancelSnooze) | Event::Timer(TimerId::SnoozeExpiry) => {
                Some(Transition::to(ctx.watch_or_active()))
            }
            Event::Control(ControlCommand::Snooze(duration)) => {
                if let Some(snooze) = ctx.snooze(*duration).and_then(|next| next.snooze) {
                    start(ctx, snooze);
                    let entry = ctx.history(HistoryKind::Snooze).with_snooze(snooze);
                    ctx.emit(Command::Record(entry));
                }
                None
            }
            event if is_input(event) && ctx.config.prompt.snooze_cancelled_by_input => {
                Some(Transition::to(State::Active))
            }
            _ => None,
        }
    }
}

fn start(ctx: &mut Ctx, duration: Duration) {
    ctx.snooze_until = ctx.now.checked_add(duration);
    ctx.set_timer(TimerId::SnoozeExpiry, duration);
}
