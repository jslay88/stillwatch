//! Paused: the user paused Stillwatch. Nothing runs until resumed, except
//! the [`ceiling`](super::ceiling) with `safety.ceiling_during_pause`.
//!
//! A pause outlasts a system sleep.

use super::{Ctx, StateHandler, Transition, ceiling};
use crate::event::{ControlCommand, Event};

pub(super) struct Handler;

fn ceiling_applies(ctx: &Ctx) -> bool {
    ctx.config.safety.ceiling_during_pause
}

impl StateHandler for Handler {
    fn enter(&self, ctx: &mut Ctx, _via: &Transition) -> Option<Transition> {
        ceiling::enter(ctx, ceiling_applies(ctx));
        None
    }

    fn exit(&self, ctx: &mut Ctx) {
        ceiling::stop(ctx);
    }

    fn on_event(&self, ctx: &mut Ctx, event: &Event) -> Option<Transition> {
        match event {
            Event::Control(ControlCommand::Resume) => Some(Transition::to(ctx.watch_or_active())),
            event => ceiling::on_event(ctx, event, ceiling_applies(ctx)),
        }
    }

    fn reconfigure(&self, ctx: &mut Ctx) {
        ceiling::refresh(ctx, ceiling_applies(ctx));
    }

    fn suspend(&self, ctx: &mut Ctx) -> Option<Transition> {
        ceiling::stop(ctx);
        None
    }
}
