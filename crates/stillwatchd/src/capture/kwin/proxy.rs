//! Client side of `org.kde.KWin.ScreenShot2`, as introspected on Plasma 6.7.5
//! (`busctl --user introspect org.kde.KWin /org/kde/KWin/ScreenShot2`):
//!
//! | Member | Signature |
//! | -- | -- |
//! | `CaptureScreen(s name, a{sv} options, h pipe)` | `-> a{sv} results` |
//! | property `Version` | `u` (5 on Plasma 6.7.5) |
//!
//! `KWin` also registers the well-known name `org.kde.KWin.ScreenShot2` for
//! the screenshot plugin, so a `KWin` without the plugin answers
//! `ServiceUnknown` rather than `UnknownObject`.

use std::collections::HashMap;

use zbus::zvariant::{Fd, OwnedValue, Value};

/// `KWin`'s screenshot interface.
#[zbus::proxy(
    interface = "org.kde.KWin.ScreenShot2",
    default_service = "org.kde.KWin.ScreenShot2",
    default_path = "/org/kde/KWin/ScreenShot2"
)]
pub trait ScreenShot2 {
    /// Renders output `name` and writes the raw image into `pipe` after
    /// replying with its metadata.
    fn capture_screen(
        &self,
        name: &str,
        options: HashMap<&str, Value<'_>>,
        pipe: Fd<'_>,
    ) -> zbus::Result<HashMap<String, OwnedValue>>;

    /// The interface revision.
    #[zbus(property)]
    fn version(&self) -> zbus::Result<u32>;
}

/// The `CaptureScreen` options Stillwatch sends: no cursor, and the output's
/// native pixel size rather than its logical (scaled) size.
pub fn capture_options() -> HashMap<&'static str, Value<'static>> {
    HashMap::from([
        ("include-cursor", Value::from(false)),
        ("native-resolution", Value::from(true)),
    ])
}
