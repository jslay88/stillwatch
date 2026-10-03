//! `[capture]`

use super::{Choice, Control, Section, Setting};

pub(super) const SECTION: Section = Section {
    id: "capture",
    title: "Capture",
    help: "How the screen is read. Frames are downscaled to a luma grid and dropped right \
           away; pixels are never stored or sent anywhere.",
    settings: &[Setting::new(
        "capture.backend",
        "Capture backend",
        Control::Enum {
            choices: &[
                Choice {
                    value: "auto",
                    label: "Automatic",
                    help: "KWin ScreenShot2 when it's available, otherwise the portal.",
                },
                Choice {
                    value: "kwin",
                    label: "KWin ScreenShot2",
                    help: "KDE's screenshot interface. No screen sharing indicator.",
                },
                Choice {
                    value: "portal",
                    label: "Desktop portal",
                    help: "xdg-desktop-portal ScreenCast. Works on any desktop. The stream \
                           only runs while you're away.",
                },
            ],
        },
        "Which capture backend reads the screen.",
    )
    .resetting_detection()],
};
