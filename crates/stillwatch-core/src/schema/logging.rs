//! `[logging]`

use super::{Choice, Control, Section, Setting};

pub(super) const SECTION: Section = Section {
    id: "logging",
    title: "Logging",
    help: "Daemon log output.",
    settings: &[Setting::new(
        "logging.level",
        "Log level",
        Control::Enum {
            choices: &[
                Choice {
                    value: "error",
                    label: "Error",
                    help: "Errors only.",
                },
                Choice {
                    value: "warn",
                    label: "Warning",
                    help: "Warnings and errors.",
                },
                Choice {
                    value: "info",
                    label: "Info",
                    help: "Informational messages and above.",
                },
                Choice {
                    value: "debug",
                    label: "Debug",
                    help: "Debug messages and above.",
                },
                Choice {
                    value: "trace",
                    label: "Trace",
                    help: "Everything.",
                },
            ],
        },
        "How much the daemon logs.",
    )],
};
