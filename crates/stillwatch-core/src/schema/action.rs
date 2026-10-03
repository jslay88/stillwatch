//! `[action]`

use super::{Choice, Control, Section, Setting, TimeUnit};
use crate::config::limits::{ANY, PERCENT};

const MODES: &[Choice] = &[
    Choice {
        value: "blank",
        label: "Blank",
        help: "Blank the displays.",
    },
    Choice {
        value: "lock_and_blank",
        label: "Lock and blank",
        help: "Lock the session, then blank.",
    },
    Choice {
        value: "dim_then_blank",
        label: "Dim, then blank",
        help: "Dim for `dim_seconds`, then blank. Input while dimmed cancels.",
    },
    Choice {
        value: "command",
        label: "Run a command",
        help: "Run `command` instead of blanking.",
    },
];

const BLANK_METHODS: &[Choice] = &[
    Choice {
        value: "dpms",
        label: "DPMS (signal off)",
        help: "Turn the signal off through the compositor. The panel reaches standby, so its \
               own compensation cycle (Pixel Cleaning on some monitors) can run.",
    },
    Choice {
        value: "overlay",
        label: "Black overlay",
        help: "Cover each output with a black surface. Works everywhere and keeps the signal \
               alive for TVs, but keeps the panel on and blocks panel compensation.",
    },
    Choice {
        value: "ddc_standby",
        label: "DDC/CI standby",
        help: "Send the standard MCCS power mode command (VCP 0xD6) over DDC/CI. Real \
               standby on monitors that support it.",
    },
];

const OUTPUTS: &[Choice] = &[
    Choice {
        value: "monitored",
        label: "Monitored outputs",
        help: "Only `stale.monitored_outputs` (every output when that's empty).",
    },
    Choice {
        value: "all",
        label: "All outputs",
        help: "Every connected output.",
    },
];

const DIM_METHODS: &[Choice] = &[
    Choice {
        value: "overlay",
        label: "Overlay",
        help: "A translucent black overlay.",
    },
    Choice {
        value: "brightness",
        label: "Brightness",
        help: "Lower the screen brightness (KDE).",
    },
];

const REBLANK_FALLBACKS: &[Choice] = &[
    Choice {
        value: "overlay",
        label: "Black overlay",
        help: "Switch to the black overlay, which keeps the signal alive so the display \
               can't wake itself.",
    },
    Choice {
        value: "none",
        label: "Give up",
        help: "Stop re-blanking.",
    },
];

pub(super) const SECTION: Section = Section {
    id: "action",
    title: "Action",
    help: "What happens when the prompt times out, plus hooks and the re-blank watchdog.",
    settings: &[
        Setting::new(
            "action.mode",
            "Action",
            Control::Enum { choices: MODES },
            "What happens when the prompt times out.",
        ),
        Setting::new(
            "action.blank_method",
            "Blank method",
            Control::Enum {
                choices: BLANK_METHODS,
            },
            "How displays are blanked. Prefer a method that reaches real standby, so the \
             panel's own care cycle can run.",
        ),
        Setting::new(
            "action.outputs",
            "Outputs to act on",
            Control::Enum { choices: OUTPUTS },
            "Which outputs the action applies to.",
        ),
        Setting::new(
            "action.dim_method",
            "Dim method",
            Control::Enum {
                choices: DIM_METHODS,
            },
            "How the dim step of `dim_then_blank` dims.",
        ),
        Setting::new(
            "action.dim_percent",
            "Dim level",
            Control::Percent { bounds: PERCENT },
            "How bright the screen stays while dimmed, as a percentage.",
        ),
        Setting::new(
            "action.dim_seconds",
            "Dim time",
            Control::Duration {
                unit: TimeUnit::Seconds,
                bounds: ANY,
            },
            "Seconds to stay dimmed before blanking.",
        ),
        Setting::new(
            "action.command",
            "Action command",
            Control::Command,
            "Command run instead of blanking with `mode = \"command\"`. Required in that mode.",
        ),
        Setting::new(
            "action.on_blank_cmd",
            "After blanking",
            Control::Command,
            "Command run after the displays are blanked, such as a TV screen-off utility. \
             Runs as `sh -c` with a 10s timeout. `STILLWATCH_OUTPUTS`, `STILLWATCH_METHOD`, \
             and `STILLWATCH_REASON` are set. Failures are logged and never block.",
        ),
        Setting::new(
            "action.on_resume_cmd",
            "On resume",
            Control::Command,
            "Command run when the displays wake. Same `sh -c` timeout and environment as \
             `on_blank_cmd` (`STILLWATCH_REASON=resume`).",
        ),
        Setting::new(
            "action.reblank_on_wake",
            "Re-blank on wake",
            Control::Toggle,
            "Blank again when a display wakes without any input, such as a monitor that \
             wakes itself when the HDMI link drops.",
        ),
        Setting::new(
            "action.reblank_grace_seconds",
            "Re-blank delay",
            Control::Duration {
                unit: TimeUnit::Seconds,
                bounds: ANY,
            },
            "Seconds to wait after an unexpected wake before blanking again.",
        ),
        Setting::new(
            "action.reblank_max_attempts",
            "Re-blank attempts",
            Control::Int {
                bounds: ANY,
                step: 1,
            },
            "Re-blank attempts before switching to `reblank_fallback`. 0 means unlimited.",
        ),
        Setting::new(
            "action.reblank_fallback",
            "After the last attempt",
            Control::Enum {
                choices: REBLANK_FALLBACKS,
            },
            "What to do when re-blanking keeps failing.",
        ),
    ],
};
