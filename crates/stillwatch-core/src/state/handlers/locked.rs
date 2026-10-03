//! Locked: the session is locked.
//!
//! Only unlocking is handled so far; the `session.when_locked` modes build on
//! this handler.

use super::{Ctx, State, StateHandler, Transition};
use crate::event::{Event, SessionEvent};

pub(super) struct Handler;

impl StateHandler for Handler {
    fn on_event(&self, _ctx: &mut Ctx, event: &Event) -> Option<Transition> {
        matches!(event, Event::Session(SessionEvent::Unlocked))
            .then(|| Transition::to(State::Active))
    }
}
