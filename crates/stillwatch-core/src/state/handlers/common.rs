//! Event handling shared by every state.

use super::super::State;
use super::super::context::{Ctx, Transition};
use crate::event::{ActivityEvent, ControlCommand, Event, SessionEvent};

/// Updates the facts an event carries. Returns `false` when the event should
/// be dropped entirely (gamepad input with `activity.gamepad` off).
pub(in crate::state) fn observe(ctx: &mut Ctx, event: &Event) -> bool {
    match event {
        Event::Activity(ActivityEvent::InputIdle) => ctx.idle = true,
        Event::Activity(ActivityEvent::InputResumed) => {
            ctx.idle = false;
            ctx.wake_displays();
        }
        Event::Activity(ActivityEvent::GamepadActivity { .. }) => {
            if !ctx.config.activity.gamepad {
                return false;
            }
            ctx.idle = false;
            ctx.last_gamepad = Some(ctx.now);
            // The compositor never sees evdev gamepad input, so only
            // Stillwatch can wake the displays for it.
            if ctx.config.activity.gamepad_wakes_display {
                ctx.wake_displays();
            }
        }
        Event::Session(SessionEvent::Locked) => ctx.locked = true,
        Event::Session(SessionEvent::Unlocked) => ctx.locked = false,
        Event::Media { playing } => ctx.playing.clone_from(playing),
        _ => {}
    }
    true
}

/// Transitions available from every state.
pub(in crate::state) fn global(state: State, event: &Event) -> Option<Transition> {
    match event {
        Event::Control(ControlCommand::Pause) if state != State::Paused => {
            Some(Transition::to(State::Paused))
        }
        _ => None,
    }
}
