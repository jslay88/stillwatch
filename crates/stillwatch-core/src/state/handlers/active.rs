//! Active: the user is present. Nothing is captured.
//!
//! The machine never stays in Active while the session is locked: entering
//! it with the session locked hands straight over to Locked.

use super::{Ctx, State, StateHandler, Transition};
use crate::event::{ActivityEvent, ControlCommand, Event, SessionEvent};

pub(super) struct Handler;

impl StateHandler for Handler {
    fn enter(&self, ctx: &mut Ctx, _via: &Transition) -> Option<Transition> {
        to_locked(ctx)
    }

    fn on_event(&self, ctx: &mut Ctx, event: &Event) -> Option<Transition> {
        match event {
            Event::Activity(ActivityEvent::InputIdle) => Some(Transition::to(State::Monitoring)),
            Event::Session(SessionEvent::Locked) => Some(Transition::to(State::Locked)),
            Event::Control(ControlCommand::Snooze(duration)) => ctx.snooze(*duration),
            _ => None,
        }
    }

    fn suspend(&self, _ctx: &mut Ctx) -> Option<Transition> {
        None
    }

    fn resume(&self, ctx: &mut Ctx) -> Option<Transition> {
        to_locked(ctx)
    }
}

fn to_locked(ctx: &Ctx) -> Option<Transition> {
    ctx.locked.then(|| Transition::to(State::Locked))
}
