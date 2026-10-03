//! Panel care beside the state machine: power reports in, reminder and
//! trigger commands out. The tracker itself is pure; this module turns its
//! answers into [`Command`]s.

use std::time::Instant;

use jiff::Timestamp;

use super::context::Ctx;
use crate::command::{Command, HookKind};
use crate::event::{Event, SessionEvent};
use crate::panel::{PanelRecord, PanelUpdate};
use crate::prompt::Reminder;
use crate::time::{Clock, TimerId};

/// The clock reading the machine already took for this step.
struct Stamp {
    now: Instant,
    wall: Timestamp,
}

impl Clock for Stamp {
    fn now(&self) -> Instant {
        self.now
    }

    fn wall_now(&self) -> Timestamp {
        self.wall
    }
}

fn stamp(ctx: &Ctx) -> Stamp {
    Stamp {
        now: ctx.now,
        wall: ctx.wall,
    }
}

pub(super) fn record(ctx: &Ctx, now: Instant) -> Option<PanelRecord> {
    ctx.panel.enabled().then(|| {
        ctx.panel.record(&Stamp {
            now,
            wall: ctx.wall,
        })
    })
}

pub(super) fn restore(ctx: &mut Ctx, saved: PanelRecord) {
    ctx.panel.restore_record(saved);
    ctx.panel_check = None;
}

pub(super) fn reconfigure(ctx: &mut Ctx) {
    let update = ctx
        .panel
        .set_config(ctx.config.panel_care.clone(), &stamp(ctx));
    apply(ctx, &update);
}

/// `trigger_cmd` at blank time, only when panel care is due.
pub(super) fn on_blank(ctx: &mut Ctx) {
    if ctx.panel.trigger_due(&stamp(ctx)) {
        ctx.hook(HookKind::PanelCareTrigger);
    }
}

/// Folds a power report or the reminder timer into commands.
///
/// Power is recorded even while asleep. Timers are not armed then: the
/// monotonic clock is stopped, and resume schedules the next check.
pub(super) fn after_event(ctx: &mut Ctx, event: &Event) {
    if matches!(event, Event::Session(SessionEvent::PrepareForSleep)) {
        cancel_check(ctx);
        return;
    }
    if ctx.asleep {
        if let Event::DisplayPower { output, on, kind } = event {
            ctx.panel.record_power(&stamp(ctx), output, *kind, *on);
        }
        return;
    }
    let update = match event {
        Event::DisplayPower { output, on, kind } => {
            ctx.panel.power(&stamp(ctx), output, *kind, *on)
        }
        Event::Timer(TimerId::PanelCareReminder) => {
            ctx.panel_check = None;
            ctx.panel.tick(&stamp(ctx))
        }
        Event::Session(SessionEvent::ResumedFromSleep) => ctx.panel.tick(&stamp(ctx)),
        _ => return,
    };
    apply(ctx, &update);
}

fn apply(ctx: &mut Ctx, update: &PanelUpdate) {
    if let Some(screen_on) = update.reminder {
        ctx.emit(Command::Notify(Reminder::PanelCare { screen_on }));
    }
    match update.check_after {
        Some(after) => {
            let deadline = ctx.now.checked_add(after);
            if ctx.panel_check != deadline {
                ctx.set_timer(TimerId::PanelCareReminder, after);
                ctx.panel_check = deadline;
            }
        }
        None => cancel_check(ctx),
    }
}

fn cancel_check(ctx: &mut Ctx) {
    if ctx.panel_check.take().is_some() {
        ctx.disarm(TimerId::PanelCareReminder);
    }
}
