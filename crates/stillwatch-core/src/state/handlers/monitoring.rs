//! Monitoring: the user is idle, so the screen is captured every
//! `stale.check_interval_seconds` and fed to the detector.

use super::{Ctx, State, StateHandler, Transition, is_input};
use crate::event::{ActivityEvent, ControlCommand, Event, SessionEvent};
use crate::time::TimerId;

pub(super) struct Handler;

impl StateHandler for Handler {
    fn enter(&self, ctx: &mut Ctx, _via: &Transition) -> Option<Transition> {
        // Counters from an earlier idle period compare against a screen the
        // user has since used, so every watch starts fresh.
        ctx.detector.reset();
        ctx.request_capture();
        ctx.arm_capture();
        None
    }

    fn exit(&self, ctx: &mut Ctx) {
        ctx.cancel_timer(TimerId::Capture);
    }

    fn on_event(&self, ctx: &mut Ctx, event: &Event) -> Option<Transition> {
        match event {
            Event::Timer(TimerId::Capture) => {
                ctx.request_capture();
                ctx.arm_capture();
                None
            }
            Event::CaptureCompleted { frames } if ctx.activity_known() => {
                let stats = ctx.observe(frames);
                stats
                    .stale
                    .then(|| Transition::to(State::Prompting).with_detection(stats))
            }
            Event::Activity(ActivityEvent::Unknown) => Some(Transition::to(State::Active)),
            Event::OutputsChanged(_) => {
                ctx.resume_capture();
                None
            }
            Event::Session(SessionEvent::Locked) => Some(Transition::to(State::Locked)),
            Event::Control(ControlCommand::Snooze(duration)) => ctx.snooze(*duration),
            event if is_input(event) => Some(Transition::to(State::Active)),
            _ => None,
        }
    }

    fn reconfigure(&self, ctx: &mut Ctx) {
        ctx.arm_capture();
    }
}
