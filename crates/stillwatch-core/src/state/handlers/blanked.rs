//! Blanked: displays are off until input.
//!
//! Waking the displays on input happens in [`common`](super::common), since
//! input can arrive in any state while outputs are blanked.
//!
//! The re-blank watchdog: with `action.reblank_on_wake`, an output Stillwatch
//! blanked that reports power on (DPMS on, or the overlay went away) with no
//! input is blanked again after `reblank_grace_seconds`. The first
//! `reblank_max_attempts` re-blanks (0 = unlimited) use the configured
//! method; after that `reblank_fallback` decides: the black overlay, which
//! keeps the signal alive, or giving up until input. Attempts count per
//! blank episode and reset on input.

use std::time::Duration;

use super::{Ctx, State, StateHandler, Transition, is_input};
use crate::command::{BlankMethod, Command, HookKind};
use crate::config::ReblankFallback;
use crate::event::Event;
use crate::history::HistoryKind;
use crate::time::TimerId;

pub(super) struct Handler;

impl StateHandler for Handler {
    fn enter(&self, ctx: &mut Ctx, _via: &Transition) -> Option<Transition> {
        ctx.hook(HookKind::OnBlank);
        super::super::care::on_blank(ctx);
        None
    }

    fn exit(&self, ctx: &mut Ctx) {
        ctx.disarm(TimerId::ReblankGrace);
    }

    fn on_event(&self, ctx: &mut Ctx, event: &Event) -> Option<Transition> {
        match event {
            Event::DisplayPower {
                output, on: true, ..
            } => {
                if woke(ctx, output)
                    && next_reblank(ctx).is_some()
                    && !ctx.is_armed(TimerId::ReblankGrace)
                {
                    let grace = ctx.config.action.reblank_grace_seconds;
                    ctx.set_timer(TimerId::ReblankGrace, Duration::from_secs(u64::from(grace)));
                }
                None
            }
            Event::Timer(TimerId::ReblankGrace) => reblank(ctx),
            event if is_input(event) => Some(Transition::to(State::Active)),
            _ => None,
        }
    }
}

/// Whether `output` is one Stillwatch blanked (an empty list means all).
fn woke(ctx: &Ctx, output: &str) -> bool {
    ctx.blanked
        .as_ref()
        .is_some_and(|outputs| outputs.is_empty() || outputs.iter().any(|name| name == output))
}

/// The method for the next re-blank and whether it's the fallback, or
/// `None` when the watchdog is off or has given up.
fn next_reblank(ctx: &Ctx) -> Option<(BlankMethod, bool)> {
    let action = &ctx.config.action;
    if !action.reblank_on_wake {
        return None;
    }
    let max = action.reblank_max_attempts;
    if max == 0 || ctx.reblank_attempts < max {
        return Some((action.blank_method, false));
    }
    match action.reblank_fallback {
        ReblankFallback::Overlay => Some((BlankMethod::Overlay, true)),
        ReblankFallback::None => None,
    }
}

fn reblank(ctx: &mut Ctx) -> Option<Transition> {
    let (method, fallback) = next_reblank(ctx)?;
    let attempt = ctx.reblank_attempts.saturating_add(1);
    ctx.reblank_attempts = attempt;
    let overlay_used = fallback.then_some(HistoryKind::OverlayUsed);
    for kind in std::iter::once(HistoryKind::Reblank).chain(overlay_used) {
        let entry = ctx
            .history(kind)
            .with_blank_method(method)
            .with_reblank_attempt(attempt);
        ctx.emit(Command::Record(entry));
    }
    Some(
        Transition::to(State::Acting)
            .with_blank_method(method)
            .with_reblank_attempt(attempt),
    )
}
