//! Event handling shared by every state.
//!
//! Suspend and resume: `PrepareForSleep` lets the current state stop its
//! timers (most states wait out the sleep in Active, while Locked and Paused
//! stay put), and every event but `ResumedFromSleep` is then dropped once
//! its facts (lock state, media, outputs) are noted. The monotonic clock
//! stops while the system sleeps, so nothing armed before sleep could be
//! trusted afterwards. On resume the user counts as present and the detector
//! starts over.

use super::super::State;
use super::super::context::{Ctx, Transition};
use super::handler;
use crate::event::{ActivityEvent, ControlCommand, Event, SessionEvent};

/// Updates the facts an event carries. Returns `false` when the event should
/// be dropped entirely (gamepad input with `activity.gamepad` off, anything
/// while asleep, or a resume without a sleep).
pub(in crate::state) fn observe(ctx: &mut Ctx, event: &Event) -> bool {
    match event {
        Event::Activity(ActivityEvent::InputIdle) => ctx.idle = true,
        Event::Activity(ActivityEvent::InputResumed) => present(ctx, true),
        Event::Activity(ActivityEvent::GamepadActivity { .. }) => {
            if !ctx.config.activity.gamepad {
                return false;
            }
            ctx.last_gamepad = Some(ctx.now);
            // The compositor never sees evdev gamepad input, so only
            // Stillwatch can wake the displays for it.
            present(ctx, ctx.config.activity.gamepad_wakes_display);
        }
        Event::Session(SessionEvent::Locked) => ctx.locked = true,
        Event::Session(SessionEvent::Unlocked) => ctx.locked = false,
        Event::Session(SessionEvent::PrepareForSleep) => {
            ctx.asleep = true;
            return true;
        }
        Event::Session(SessionEvent::ResumedFromSleep) => {
            if !ctx.asleep {
                return false;
            }
            ctx.asleep = false;
            ctx.idle = false;
            ctx.detector.reset();
            return true;
        }
        Event::Media { playing } => ctx.playing.clone_from(playing),
        Event::OutputsChanged(outputs) => ctx.detector.set_outputs(outputs),
        Event::Timer(id) => ctx.fired(*id),
        _ => {}
    }
    !ctx.asleep
}

/// Input: the user is back, re-blanks start counting again, and outputs
/// Stillwatch blanked wake if this input may wake them.
fn present(ctx: &mut Ctx, wakes_displays: bool) {
    ctx.idle = false;
    ctx.reblank_attempts = 0;
    if wakes_displays {
        ctx.wake_displays();
    }
}

/// Transitions available from every state.
pub(in crate::state) fn global(state: State, ctx: &mut Ctx, event: &Event) -> Option<Transition> {
    match event {
        Event::Control(ControlCommand::Pause) if state != State::Paused => {
            Some(Transition::to(State::Paused))
        }
        Event::Session(SessionEvent::PrepareForSleep) => handler(state).suspend(ctx),
        Event::Session(SessionEvent::ResumedFromSleep) => handler(state).resume(ctx),
        _ => None,
    }
}
