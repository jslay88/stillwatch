//! `[history]`

use super::{Control, Section, Setting};
use crate::config::limits::POSITIVE;

pub(super) const SECTION: Section = Section {
    id: "history",
    title: "History",
    help: "The decision history behind `stillwatch history` and the History page. It holds \
           numbers and state names only: no pixels, window titles, or track metadata.",
    settings: &[
        Setting::new(
            "history.enabled",
            "Record history",
            Control::Toggle,
            "Record every prompt, blank, snooze, ceiling trigger, re-blank, and reload, with \
             the numbers behind it.",
        ),
        Setting::new(
            "history.max_entries",
            "Entries kept",
            Control::Int {
                bounds: POSITIVE,
                step: 100,
            },
            "Entries kept in `~/.local/state/stillwatch/history.jsonl`. The oldest are \
             dropped first.",
        ),
    ],
};
