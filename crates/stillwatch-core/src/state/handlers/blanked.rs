//! Blanked: displays are off until input.
//!
//! Waking the displays on input happens in [`common`](super::common), since
//! input can arrive in any state while outputs are blanked. The re-blank
//! watchdog (`Event::DisplayPower` with no input) isn't handled yet.

use super::{Ctx, State, StateHandler, Transition, is_input};
use crate::command::HookKind;
use crate::event::Event;

pub(super) struct Handler;

impl StateHandler for Handler {
    fn enter(&self, ctx: &mut Ctx, _via: &Transition) -> Option<Transition> {
        ctx.hook(HookKind::OnBlank);
        None
    }

    fn on_event(&self, _ctx: &mut Ctx, event: &Event) -> Option<Transition> {
        is_input(event).then(|| Transition::to(State::Active))
    }
}
