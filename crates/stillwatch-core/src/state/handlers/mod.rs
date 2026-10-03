//! One handler per state.
//!
//! A handler reacts to events in its state and returns the [`Transition`] it
//! wants, if any. Entering and leaving a state run its `enter` and `exit`
//! hooks, which is where timers are armed and disarmed. Events every state
//! treats the same way are handled in [`common`] before the handler runs.

mod acting;
mod active;
mod blanked;
pub(super) mod common;
mod locked;
mod monitoring;
mod paused;
mod prompting;
mod snoozed;

use super::State;
use super::context::{Ctx, Transition};
use crate::event::{ActivityEvent, Event};

/// Behavior for one state.
pub(super) trait StateHandler: Sync {
    /// Runs on entry. May return a follow-up transition, for example an
    /// action that finishes as soon as it starts.
    fn enter(&self, _ctx: &mut Ctx, _via: &Transition) -> Option<Transition> {
        None
    }

    /// Runs on exit, before the next state's `enter`.
    fn exit(&self, _ctx: &mut Ctx) {}

    /// Reacts to an event while in this state.
    fn on_event(&self, ctx: &mut Ctx, event: &Event) -> Option<Transition>;

    /// Runs after a config reload while in this state.
    fn reconfigure(&self, _ctx: &mut Ctx) {}
}

/// The handler for `state`.
pub(super) fn handler(state: State) -> &'static dyn StateHandler {
    match state {
        State::Active => &active::Handler,
        State::Monitoring => &monitoring::Handler,
        State::Prompting => &prompting::Handler,
        State::Snoozed => &snoozed::Handler,
        State::Acting => &acting::Handler,
        State::Blanked => &blanked::Handler,
        State::Locked => &locked::Handler,
        State::Paused => &paused::Handler,
    }
}

/// Keyboard, mouse, or (enabled) gamepad input.
const fn is_input(event: &Event) -> bool {
    matches!(
        event,
        Event::Activity(ActivityEvent::InputResumed | ActivityEvent::GamepadActivity { .. })
    )
}
