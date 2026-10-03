//! Active: the user is present. Nothing is captured.

use super::{Ctx, State, StateHandler, Transition};
use crate::event::{ActivityEvent, ControlCommand, Event, SessionEvent};

pub(super) struct Handler;

impl StateHandler for Handler {
    fn on_event(&self, ctx: &mut Ctx, event: &Event) -> Option<Transition> {
        match event {
            Event::Activity(ActivityEvent::InputIdle) => Some(Transition::to(State::Monitoring)),
            Event::Session(SessionEvent::Locked) => Some(Transition::to(State::Locked)),
            Event::Control(ControlCommand::Snooze(duration)) => ctx.snooze(*duration),
            _ => None,
        }
    }
}
