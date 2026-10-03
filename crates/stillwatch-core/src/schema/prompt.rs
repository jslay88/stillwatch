//! `[prompt]`

use super::{Choice, Control, Section, Setting, TimeUnit};
use crate::config::limits::{ANY, POSITIVE};

const STYLES: &[Choice] = &[
    Choice {
        value: "auto",
        label: "Automatic",
        help: "A notification normally, the dialog when a notification wouldn't be seen.",
    },
    Choice {
        value: "notification",
        label: "Notification",
        help: "A desktop notification with snooze actions and a live countdown.",
    },
    Choice {
        value: "dialog",
        label: "Dialog",
        help: "Stillwatch's own dialog window.",
    },
];

const URGENCIES: &[Choice] = &[
    Choice {
        value: "low",
        label: "Low",
        help: "Low urgency.",
    },
    Choice {
        value: "normal",
        label: "Normal",
        help: "Normal urgency.",
    },
    Choice {
        value: "critical",
        label: "Critical",
        help: "Critical urgency, which Plasma shows even in Do Not Disturb.",
    },
];

pub(super) const SECTION: Section = Section {
    id: "prompt",
    title: "Prompt",
    help: "The prompt shown before acting, with its countdown and snooze choices.",
    settings: &[
        Setting::new(
            "prompt.style",
            "Prompt style",
            Control::Enum { choices: STYLES },
            "How the prompt is shown.",
        ),
        Setting::new(
            "prompt.urgency",
            "Notification urgency",
            Control::Enum { choices: URGENCIES },
            "Urgency of the prompt notification. Critical keeps it visible in Do Not Disturb.",
        ),
        Setting::new(
            "prompt.fallback_to_dialog",
            "Fall back to the dialog",
            Control::Toggle,
            "Show the dialog when there's no notification server, the notification fails, \
             or it's closed without picking an action.",
        ),
        Setting::new(
            "prompt.countdown_seconds",
            "Countdown",
            Control::Duration {
                unit: TimeUnit::Seconds,
                bounds: POSITIVE,
            },
            "Seconds the prompt waits for an answer before the action runs. Any input \
             cancels it.",
        ),
        Setting::new(
            "prompt.snooze_presets_minutes",
            "Snooze buttons",
            Control::IntList {
                unit: Some(TimeUnit::Minutes),
                bounds: POSITIVE,
            },
            "Snooze choices on the prompt, in minutes. Each must be between \
             `custom_min_minutes` and `custom_max_minutes`. Can only be empty when \
             `allow_custom` is on.",
        ),
        Setting::new(
            "prompt.allow_custom",
            "Custom snooze",
            Control::Toggle,
            "Offer a Custom... choice that opens the dialog to pick any duration.",
        ),
        Setting::new(
            "prompt.custom_min_minutes",
            "Shortest snooze",
            Control::Duration {
                unit: TimeUnit::Minutes,
                bounds: POSITIVE,
            },
            "Shortest snooze allowed, in minutes.",
        ),
        Setting::new(
            "prompt.custom_max_minutes",
            "Longest snooze",
            Control::Duration {
                unit: TimeUnit::Minutes,
                bounds: ANY,
            },
            "Longest snooze allowed, in minutes. Must be at least `custom_min_minutes`.",
        ),
        Setting::new(
            "prompt.snooze_cancelled_by_input",
            "Input ends a snooze",
            Control::Toggle,
            "End a snooze as soon as there's input. Off keeps the snooze for the whole time \
             you picked.",
        ),
    ],
};
