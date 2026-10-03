//! The snooze ceiling, shared by Snoozed and (with
//! `safety.ceiling_during_pause`) Paused.
//!
//! While the user is idle these states keep capturing every
//! `stale.check_interval_seconds`, and the detector measures the same block
//! counters over `safety.ceiling_minutes`. When the ceiling verdict is stale
//! the machine prompts, so a long snooze can't hide a fully static screen.
//! `applies` is the state's own switch: always on for Snoozed,
//! `ceiling_during_pause` for Paused.

use super::{Ctx, State, Transition, is_input};
use crate::command::Command;
use crate::event::{ActivityEvent, CaptureFrame, Event};
use crate::history::HistoryKind;
use crate::time::TimerId;

fn wanted(ctx: &Ctx, applies: bool) -> bool {
    applies && ctx.config.safety.ceiling_enabled && ctx.idle && !ctx.asleep
}

/// On entry. The counters carry on from Monitoring, since the user has
/// stayed idle and the screen hasn't been touched since.
pub(super) fn enter(ctx: &mut Ctx, applies: bool) {
    if wanted(ctx, applies) {
        ctx.arm_capture();
    }
}

/// Stops capturing.
pub(super) fn stop(ctx: &mut Ctx) {
    ctx.disarm(TimerId::Capture);
}

/// After a reload: re-arm with the new interval, start, or stop.
pub(super) fn refresh(ctx: &mut Ctx, applies: bool) {
    if !wanted(ctx, applies) {
        stop(ctx);
    } else if ctx.is_armed(TimerId::Capture) {
        ctx.arm_capture();
    } else {
        start_fresh(ctx);
    }
}

/// Handles idle, input, capture ticks, and capture results.
pub(super) fn on_event(ctx: &mut Ctx, event: &Event, applies: bool) -> Option<Transition> {
    match event {
        Event::Activity(ActivityEvent::InputIdle) => {
            if wanted(ctx, applies) && !ctx.is_armed(TimerId::Capture) {
                start_fresh(ctx);
            }
            None
        }
        Event::Timer(TimerId::Capture) if wanted(ctx, applies) => {
            ctx.request_capture();
            ctx.arm_capture();
            None
        }
        Event::CaptureCompleted { frames } if wanted(ctx, applies) => check(ctx, frames),
        event if is_input(event) => {
            stop(ctx);
            None
        }
        _ => None,
    }
}

/// A new idle period: the counters compare against a screen the user has
/// since used, so they start over.
fn start_fresh(ctx: &mut Ctx) {
    ctx.detector.reset();
    ctx.request_capture();
    ctx.arm_capture();
}

fn check(ctx: &mut Ctx, frames: &[CaptureFrame]) -> Option<Transition> {
    ctx.observe(frames);
    let stats = ctx.detector.ceiling().filter(|stats| stats.stale)?;
    let entry = ctx
        .history(HistoryKind::Ceiling)
        .with_detection(stats.clone());
    ctx.emit(Command::Record(entry));
    Some(Transition::to(State::Prompting).with_detection(stats))
}
