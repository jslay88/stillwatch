//! `[panel_care]`

use super::{Control, Section, Setting, TimeUnit};
use crate::config::limits::{ANY, POSITIVE};

pub(super) const SECTION: Section = Section {
    id: "panel_care",
    title: "Panel care",
    help: "Helps the display's own compensation cycle run, which most OLED panels do in \
           standby after cumulative use. Stillwatch never draws pixel-exercise patterns: \
           lighting pixels only adds wear.",
    settings: &[
        Setting::new(
            "panel_care.enabled",
            "Track screen-on time",
            Control::Toggle,
            "Track how long the displays have been on since their last real standby.",
        ),
        Setting::new(
            "panel_care.min_standby_minutes",
            "Standby that resets",
            Control::Duration {
                unit: TimeUnit::Minutes,
                bounds: ANY,
            },
            "Minutes in standby that reset screen-on time. The black overlay doesn't count, \
             since the panel stays on.",
        ),
        Setting::new(
            "panel_care.reminder_enabled",
            "Reminder",
            Control::Toggle,
            "Once screen-on time passes `reminder_hours`, remind you that turning the \
             display off lets panel care run.",
        ),
        Setting::new(
            "panel_care.reminder_hours",
            "Remind after",
            Control::Duration {
                unit: TimeUnit::Hours,
                bounds: POSITIVE,
            },
            "Screen-on hours before the reminder.",
        ),
        Setting::new(
            "panel_care.trigger_cmd",
            "Panel care command",
            Control::Command,
            "Command run at blank time when panel care is due, for displays with a \
             model-specific command. No vendor codes are built in, because sending \
             undocumented codes is unsafe.",
        ),
    ],
};
