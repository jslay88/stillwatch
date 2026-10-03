//! What a probe saw. Selection is a pure function of this plus config.

use wayland_client::globals::Global;

/// Wayland globals a later wlr capture backend will fill in.
///
/// JUS-48 owns `zwlr_screencopy_manager_v1` and
/// `ext_image_copy_capture_manager_v1`. [`read_wlr_capture`] is where that
/// probe records them. It returns the default until then, and selection
/// does not read the value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WlrCaptureGlobals {
    /// `zwlr_screencopy_manager_v1`, once JUS-48 probes it.
    pub screencopy_manager: bool,
    /// `ext_image_copy_capture_manager_v1`, once JUS-48 probes it.
    pub image_copy_capture: bool,
}

/// Extension point for JUS-48. Does not look at `globals` yet.
#[must_use]
pub fn read_wlr_capture(globals: &[Global]) -> WlrCaptureGlobals {
    let _ = globals;
    WlrCaptureGlobals::default()
}

/// Wayland globals the selector cares about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaylandFacts {
    /// Advertised version of `ext_idle_notifier_v1`, if the compositor has it.
    pub idle_notifier_version: Option<u32>,
    /// `org_kde_kwin_dpms_manager` is advertised.
    pub kwin_dpms: bool,
    /// `zwlr_layer_shell_v1` is advertised.
    pub layer_shell: bool,
    /// Reserved for JUS-48. Always the default from today's probe.
    pub wlr_capture: WlrCaptureGlobals,
}

/// `org.kde.KWin` and its screenshot interface.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KwinBus {
    /// The `org.kde.KWin` name has an owner.
    pub present: bool,
    /// `org.kde.KWin.ScreenShot2` has an owner.
    pub screenshot2: bool,
    /// A `.desktop` grant authorizes this executable for `ScreenShot2`.
    pub screenshot2_authorized: bool,
    /// The interface's `Version` property, when it answered.
    pub screenshot2_version: Option<u32>,
}

/// logind and the screensaver service.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionServices {
    /// `org.freedesktop.login1` on the system bus.
    pub logind: bool,
    /// `org.freedesktop.ScreenSaver` on the session bus.
    pub screensaver: bool,
}

/// D-Bus names the selector cares about.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DbusFacts {
    /// `KWin`'s bus names and `ScreenShot2`.
    pub kwin: KwinBus,
    /// `org.freedesktop.portal.Desktop` serves `ScreenCast`.
    pub portal_screencast: bool,
    /// `org.freedesktop.Notifications` has an owner.
    pub notifications: bool,
    /// logind and the screensaver.
    pub session: SessionServices,
}

/// Local programs and devices, not bus names.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ToolFacts {
    /// `kscreen-doctor` is on `PATH`.
    pub kscreen_doctor: bool,
    /// At least one GPU i2c bus can be opened.
    pub i2c: bool,
}

/// One probe of the session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    /// Wayland globals.
    pub wayland: WaylandFacts,
    /// D-Bus names.
    pub dbus: DbusFacts,
    /// Programs and device nodes.
    pub tools: ToolFacts,
}

impl Probe {
    /// Nothing is available. Useful as a base for tests.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            wayland: WaylandFacts {
                idle_notifier_version: None,
                kwin_dpms: false,
                layer_shell: false,
                wlr_capture: WlrCaptureGlobals::default(),
            },
            dbus: DbusFacts::default(),
            tools: ToolFacts::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{WlrCaptureGlobals, read_wlr_capture};

    #[test]
    fn wlr_capture_globals_stay_unset() {
        assert_eq!(read_wlr_capture(&[]), WlrCaptureGlobals::default());
    }
}
