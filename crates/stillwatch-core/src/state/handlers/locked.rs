//! Locked: the session is locked.
//!
//! With `session.when_locked = "blank_after"` the displays are blanked once
//! the session has been locked for `locked_blank_seconds`, with no stale
//! check or prompt. Input restarts that delay, so a lock screen woken by
//! input is blanked again if the session stays locked. With `"pause"` the
//! machine just waits for unlock.

use std::time::Duration;

use super::{Ctx, State, StateHandler, Transition, is_input};
use crate::config::WhenLocked;
use crate::event::{ActivityEvent, Event, SessionEvent};
use crate::time::TimerId;

fn blanks(ctx: &Ctx) -> bool {
    ctx.config.session.when_locked == WhenLocked::BlankAfter
}

/// (Re)starts the locked blank delay, unless the mode is `pause` or the
/// system is going to sleep.
fn arm(ctx: &mut Ctx) {
    if blanks(ctx) && !ctx.asleep && ctx.activity_known() {
        let delay = Duration::from_secs(u64::from(ctx.config.session.locked_blank_seconds));
        ctx.set_timer(TimerId::LockedBlank, delay);
    }
}

pub(super) struct Handler;

impl StateHandler for Handler {
    fn enter(&self, ctx: &mut Ctx, _via: &Transition) -> Option<Transition> {
        arm(ctx);
        None
    }

    fn exit(&self, ctx: &mut Ctx) {
        ctx.disarm(TimerId::LockedBlank);
    }

    fn on_event(&self, ctx: &mut Ctx, event: &Event) -> Option<Transition> {
        match event {
            Event::Session(SessionEvent::Unlocked) => Some(Transition::to(State::Active)),
            Event::Timer(TimerId::LockedBlank) if ctx.activity_known() => {
                Some(Transition::to(State::Acting))
            }
            Event::Activity(ActivityEvent::Unknown) => {
                ctx.disarm(TimerId::LockedBlank);
                None
            }
            Event::Activity(ActivityEvent::InputIdle) if !ctx.is_armed(TimerId::LockedBlank) => {
                arm(ctx);
                None
            }
            event if is_input(event) => {
                arm(ctx);
                None
            }
            _ => None,
        }
    }

    fn reconfigure(&self, ctx: &mut Ctx) {
        if !blanks(ctx) {
            ctx.disarm(TimerId::LockedBlank);
        } else if !ctx.is_armed(TimerId::LockedBlank) {
            arm(ctx);
        }
    }

    fn suspend(&self, ctx: &mut Ctx) -> Option<Transition> {
        ctx.disarm(TimerId::LockedBlank);
        None
    }

    fn resume(&self, ctx: &mut Ctx) -> Option<Transition> {
        if !ctx.locked {
            return Some(Transition::to(State::Active));
        }
        arm(ctx);
        None
    }
}
