//! `[activity]`

use super::{Control, Section, Setting};
use crate::config::limits::PERCENT;

pub(super) const SECTION: Section = Section {
    id: "activity",
    title: "Gamepads",
    help: "Gamepad input as activity. Compositors don't count gamepads as input, so \
           Stillwatch reads joystick devices itself.",
    settings: &[
        Setting::new(
            "activity.gamepad",
            "Gamepad counts as input",
            Control::Toggle,
            "Count gamepad buttons and sticks as activity, so playing with a controller \
             doesn't look like being away.",
        ),
        Setting::new(
            "activity.gamepad_deadzone_percent",
            "Stick deadzone",
            Control::Percent { bounds: PERCENT },
            "Stick movement below this percentage of full travel is ignored, so a drifting \
             stick doesn't keep you active forever.",
        ),
        Setting::new(
            "activity.gamepad_ignore_devices",
            "Ignored gamepads",
            Control::GamepadPicker,
            "Gamepads to ignore, matched by name substring. Use it for a pad with bad drift or \
             a sim rig that reports all the time.",
        ),
        Setting::new(
            "activity.gamepad_wakes_display",
            "Gamepad wakes displays",
            Control::Toggle,
            "Wake blanked displays on gamepad input. The compositor only wakes them for \
             keyboard and mouse, so Stillwatch does it itself.",
        ),
    ],
};
