//! `[safety]`

use super::{Control, Section, Setting, TimeUnit};
use crate::config::limits::{ANY, PERCENT_NONZERO};

pub(super) const SECTION: Section = Section {
    id: "safety",
    title: "Snooze ceiling",
    help: "A backstop for a snooze left running over a static screen. Captures keep \
           running while you're idle during a snooze, and the prompt comes back if the \
           screen has been static for too long.",
    settings: &[
        Setting::new(
            "safety.ceiling_enabled",
            "Snooze ceiling",
            Control::Toggle,
            "Let the ceiling end a snooze early.",
        ),
        Setting::new(
            "safety.ceiling_minutes",
            "Ceiling time",
            Control::Duration {
                unit: TimeUnit::Minutes,
                bounds: ANY,
            },
            "Minutes blocks must stay unchanged during a snooze before the prompt comes \
             back. Must be longer than the normal path \
             (`persist_checks * check_interval_seconds`).",
        ),
        Setting::new(
            "safety.ceiling_stale_percent",
            "Ceiling threshold",
            Control::Percent {
                bounds: PERCENT_NONZERO,
            },
            "Percentage of counted blocks that must have been unchanged for \
             `ceiling_minutes`. Higher than `stale_percent`, because it overrides a snooze \
             you asked for.",
        ),
        Setting::new(
            "safety.ceiling_during_pause",
            "Ceiling while paused",
            Control::Toggle,
            "Apply the ceiling while paused too, not just while snoozed.",
        ),
    ],
};
