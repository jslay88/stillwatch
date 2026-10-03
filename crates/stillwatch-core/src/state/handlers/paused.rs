//! Paused: the user paused Stillwatch. Nothing runs until resumed.

use super::{Ctx, StateHandler, Transition};
use crate::event::{ControlCommand, Event};

pub(super) struct Handler;

impl StateHandler for Handler {
    fn on_event(&self, ctx: &mut Ctx, event: &Event) -> Option<Transition> {
        matches!(event, Event::Control(ControlCommand::Resume))
            .then(|| Transition::to(ctx.watch_or_active()))
    }
}
