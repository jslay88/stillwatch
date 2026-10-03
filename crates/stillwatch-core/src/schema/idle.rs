//! `[idle]`

use super::{Control, Section, Setting, TimeUnit};
use crate::config::limits::POSITIVE;

pub(super) const SECTION: Section = Section {
    id: "idle",
    title: "Idle",
    help: "When you count as away.",
    settings: &[Setting::new(
        "idle.input_idle_minutes",
        "Input idle time",
        Control::Duration {
            unit: TimeUnit::Minutes,
            bounds: POSITIVE,
        },
        "Minutes without keyboard, mouse, or gamepad input before Stillwatch starts checking \
         the screen. This is real input idle from the compositor, so apps holding idle \
         inhibitors (video players, browsers, calls) don't stop it from counting.",
    )],
};
