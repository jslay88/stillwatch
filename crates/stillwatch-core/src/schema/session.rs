//! `[session]`

use super::{Choice, Control, Section, Setting, TimeUnit};
use crate::config::limits::ANY;

pub(super) const SECTION: Section = Section {
    id: "session",
    title: "Locked session",
    help: "What happens while the session is locked.",
    settings: &[
        Setting::new(
            "session.when_locked",
            "When locked",
            Control::Enum {
                choices: &[
                    Choice {
                        value: "pause",
                        label: "Wait for unlock",
                        help: "Do nothing until you unlock. Blanking is left to the desktop's \
                               own lock screen settings.",
                    },
                    Choice {
                        value: "blank_after",
                        label: "Blank after a delay",
                        help: "Blank once the session has been locked for \
                               `locked_blank_seconds`, with no stale check or prompt. The lock \
                               screen is static by design.",
                    },
                ],
            },
            "What to do while the session is locked.",
        ),
        Setting::new(
            "session.locked_blank_seconds",
            "Blank after locked for",
            Control::Duration {
                unit: TimeUnit::Seconds,
                bounds: ANY,
            },
            "Seconds the session has to stay locked before blanking, with \
             `when_locked = \"blank_after\"`. Input while locked wakes the display, and it \
             blanks again after the same delay if the session stays locked.",
        ),
    ],
};
