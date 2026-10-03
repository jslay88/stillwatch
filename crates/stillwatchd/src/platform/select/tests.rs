use std::path::Path;

use stillwatch_core::command::BlankMethod;
use stillwatch_core::config::{CaptureBackend, Config, PromptStyle};
use stillwatch_core::history::{HistoryKind, PromptMedium};

use super::{CaptureChoice, CaptureError, IdleError, select};
use crate::platform::WlrCaptureGlobals as PublicWlr;
use crate::platform::facts::{KwinBus, Probe, SessionServices, ToolFacts, WlrCaptureGlobals};

fn probe() -> Probe {
    let mut probe = Probe::empty();
    probe.wayland.idle_notifier_version = Some(2);
    probe.wayland.kwin_dpms = true;
    probe.wayland.layer_shell = true;
    probe.dbus.kwin = KwinBus {
        present: true,
        screenshot2: true,
        screenshot2_authorized: true,
        screenshot2_version: Some(5),
    };
    probe.dbus.portal_screencast = true;
    probe.dbus.notifications = true;
    probe.dbus.session = SessionServices {
        logind: true,
        screensaver: true,
    };
    probe.tools = ToolFacts {
        kscreen_doctor: true,
        i2c: true,
    };
    probe
}

fn config(backend: CaptureBackend, blank: BlankMethod, style: PromptStyle) -> Config {
    let mut config = Config::default();
    config.capture.backend = backend;
    config.action.blank_method = blank;
    config.prompt.style = style;
    config
}

#[test]
fn a_full_kde_session_picks_kwin_dpms_and_a_notification() {
    let selection = select(
        &probe(),
        &config(CaptureBackend::Auto, BlankMethod::Dpms, PromptStyle::Auto),
    );
    assert_eq!(selection.idle_version, Some(2));
    assert!(selection.idle_error.is_none());
    assert_eq!(selection.capture, CaptureChoice::Kwin { version: Some(5) });
    assert_eq!(
        selection.capture_reason,
        "KWin ScreenShot2 is present and authorized"
    );
    assert!(selection.capture_error.is_none());
    assert_eq!(selection.blank.method, BlankMethod::Dpms);
    assert!(!selection.blank.fell_back);
    assert_eq!(selection.prompt.medium, PromptMedium::Notification);
    assert_eq!(selection.prompt.reason, "auto");
    assert_eq!(selection.capture_backend_name().as_deref(), Some("kwin"));
    assert_eq!(
        selection.history_names(),
        "ext-idle-notify-v2 kwin dpms notification"
    );
    let entry = selection.history_entry(jiff::Timestamp::from_second(1_700_000_000).unwrap());
    assert_eq!(entry.kind, HistoryKind::Backends);
    assert_eq!(
        entry.backends.as_deref(),
        Some(selection.history_names().as_str())
    );
    assert!(
        selection
            .startup_failure(Path::new("stillwatchd"))
            .is_none()
    );
    let report = selection.report();
    assert_eq!(report.capture, "kwin");
    assert_eq!(report.blank, "dpms");
    assert_eq!(report.prompt, "notification");
}

#[test]
fn auto_falls_through_capture_and_forced_backends_error() {
    let mut bare = probe();
    bare.dbus.kwin.screenshot2_authorized = false;
    let portal = select(
        &bare,
        &config(CaptureBackend::Auto, BlankMethod::Dpms, PromptStyle::Auto),
    );
    assert_eq!(portal.capture, CaptureChoice::Portal);
    assert!(portal.capture_reason.contains("portal"));
    assert!(portal.startup_failure(Path::new("stillwatchd")).is_none());

    bare.dbus.portal_screencast = false;
    let idle = select(
        &bare,
        &config(CaptureBackend::Auto, BlankMethod::Dpms, PromptStyle::Auto),
    );
    assert_eq!(idle.capture, CaptureChoice::InputIdleOnly);
    assert_eq!(idle.capture_name(), "input-idle-only");
    assert!(idle.capture_backend_name().is_none());

    let missing = probe_without_screenshot();
    let forced = select(
        &missing,
        &config(CaptureBackend::Kwin, BlankMethod::Dpms, PromptStyle::Auto),
    );
    assert_eq!(forced.capture, CaptureChoice::Unavailable);
    assert_eq!(forced.capture_error, Some(CaptureError::KwinMissing));
    assert!(
        forced
            .startup_failure(Path::new("stillwatchd"))
            .unwrap()
            .contains("isn't available")
    );

    let unauthorized = select(
        &bare,
        &config(CaptureBackend::Kwin, BlankMethod::Dpms, PromptStyle::Auto),
    );
    assert_eq!(
        unauthorized.capture_error,
        Some(CaptureError::KwinUnauthorized)
    );
    let message = unauthorized
        .startup_failure(Path::new("/usr/bin/stillwatchd"))
        .unwrap();
    assert!(message.contains("isn't authorized"));
    assert!(message.contains("X-KDE-DBUS-Restricted-Interfaces"));

    let ready = select(
        &probe(),
        &config(CaptureBackend::Kwin, BlankMethod::Dpms, PromptStyle::Auto),
    );
    assert!(matches!(ready.capture, CaptureChoice::Kwin { .. }));
    assert_eq!(ready.capture_reason, "capture.backend is kwin");

    let no_portal = probe_without_screenshot();
    let portal_missing = select(
        &no_portal,
        &config(CaptureBackend::Portal, BlankMethod::Dpms, PromptStyle::Auto),
    );
    assert_eq!(
        portal_missing.capture_error,
        Some(CaptureError::PortalMissing)
    );

    let forced_portal = select(
        &probe(),
        &config(CaptureBackend::Portal, BlankMethod::Dpms, PromptStyle::Auto),
    );
    assert_eq!(forced_portal.capture, CaptureChoice::Portal);
    assert_eq!(forced_portal.capture_reason, "capture.backend is portal");
}

fn probe_without_screenshot() -> Probe {
    let mut probe = probe();
    probe.dbus.kwin = KwinBus::default();
    probe.dbus.portal_screencast = false;
    probe
}

#[test]
fn idle_v1_and_a_missing_notifier_fail() {
    let mut v1 = probe();
    v1.wayland.idle_notifier_version = Some(1);
    let selection = select(
        &v1,
        &config(CaptureBackend::Auto, BlankMethod::Dpms, PromptStyle::Auto),
    );
    assert_eq!(selection.idle_error, Some(IdleError::V1Only));
    assert!(
        selection
            .startup_failure(Path::new("x"))
            .unwrap()
            .contains("v1")
    );

    let mut missing = probe();
    missing.wayland.idle_notifier_version = None;
    let selection = select(
        &missing,
        &config(CaptureBackend::Auto, BlankMethod::Dpms, PromptStyle::Auto),
    );
    assert_eq!(selection.idle_error, Some(IdleError::Missing));
    assert_eq!(selection.idle_label(), "ext-idle-notify missing");
    assert!(selection.history_names().starts_with("idle-missing"));
}

#[test]
fn blank_falls_back_to_overlay_only_when_layer_shell_exists() {
    let mut no_dpms = probe();
    no_dpms.tools.kscreen_doctor = false;
    let fallback = select(
        &no_dpms,
        &config(CaptureBackend::Auto, BlankMethod::Dpms, PromptStyle::Auto),
    );
    assert_eq!(fallback.blank.method, BlankMethod::Overlay);
    assert!(fallback.blank.fell_back);
    assert_eq!(fallback.blank_override(), Some(BlankMethod::Overlay));
    assert!(fallback.blank.reason.contains("overlay"));

    no_dpms.wayland.kwin_dpms = false;
    no_dpms.tools.kscreen_doctor = true;
    let still = select(
        &no_dpms,
        &config(CaptureBackend::Auto, BlankMethod::Dpms, PromptStyle::Auto),
    );
    assert!(still.blank.fell_back);

    let mut no_i2c = probe();
    no_i2c.tools.i2c = false;
    let ddc = select(
        &no_i2c,
        &config(
            CaptureBackend::Auto,
            BlankMethod::DdcStandby,
            PromptStyle::Auto,
        ),
    );
    assert_eq!(ddc.blank.method, BlankMethod::Overlay);
    assert!(ddc.blank.fell_back);

    let ddc_ok = select(
        &probe(),
        &config(
            CaptureBackend::Auto,
            BlankMethod::DdcStandby,
            PromptStyle::Auto,
        ),
    );
    assert_eq!(ddc_ok.blank.method, BlankMethod::DdcStandby);
    assert!(!ddc_ok.blank.fell_back);

    let mut nowhere = probe();
    nowhere.tools = ToolFacts::default();
    nowhere.wayland.kwin_dpms = false;
    nowhere.wayland.layer_shell = false;
    let stuck = select(
        &nowhere,
        &config(CaptureBackend::Auto, BlankMethod::Dpms, PromptStyle::Auto),
    );
    assert_eq!(stuck.blank.method, BlankMethod::Dpms);
    assert!(!stuck.blank.fell_back);
    assert!(stuck.blank.reason.contains("layer-shell"));

    let overlay_missing = select(
        &nowhere,
        &config(
            CaptureBackend::Auto,
            BlankMethod::Overlay,
            PromptStyle::Auto,
        ),
    );
    assert_eq!(overlay_missing.blank.method, BlankMethod::Overlay);
    assert!(!overlay_missing.blank.fell_back);
    assert_eq!(overlay_missing.blank.reason, "layer-shell isn't available");
}

#[test]
fn prompt_uses_the_dialog_only_when_notifications_are_missing() {
    let notified = select(
        &probe(),
        &config(CaptureBackend::Auto, BlankMethod::Dpms, PromptStyle::Auto),
    );
    assert_eq!(notified.prompt.medium, PromptMedium::Notification);
    assert_eq!(notified.prompt.reason, "auto");

    let dialog = select(
        &probe(),
        &config(CaptureBackend::Auto, BlankMethod::Dpms, PromptStyle::Dialog),
    );
    assert_eq!(dialog.prompt.medium, PromptMedium::Dialog);
    assert_eq!(dialog.prompt.reason, "configured");

    let forced = select(
        &probe(),
        &config(
            CaptureBackend::Auto,
            BlankMethod::Dpms,
            PromptStyle::Notification,
        ),
    );
    assert_eq!(forced.prompt.reason, "configured");

    let mut quiet = probe();
    quiet.dbus.notifications = false;
    for style in [PromptStyle::Auto, PromptStyle::Notification] {
        let selection = select(
            &quiet,
            &config(CaptureBackend::Auto, BlankMethod::Dpms, style),
        );
        assert_eq!(selection.prompt.medium, PromptMedium::Dialog);
        assert_eq!(selection.prompt.reason, "notifications aren't available");
    }
}

#[test]
fn wlr_capture_globals_do_not_change_the_choice() {
    let mut probe = probe();
    probe.dbus.kwin.screenshot2_authorized = false;
    probe.wayland.wlr_capture = WlrCaptureGlobals {
        screencopy_manager: true,
        image_copy_capture: true,
    };
    let selection = select(
        &probe,
        &config(
            CaptureBackend::Auto,
            BlankMethod::Overlay,
            PromptStyle::Auto,
        ),
    );
    assert_eq!(selection.capture, CaptureChoice::Portal);
    let defaults = PublicWlr::default();
    assert!(!defaults.screencopy_manager);
    assert!(!defaults.image_copy_capture);
}

#[test]
fn a_newer_idle_notifier_is_accepted() {
    let mut probe = probe();
    probe.wayland.idle_notifier_version = Some(4);
    let selection = select(
        &probe,
        &config(
            CaptureBackend::Auto,
            BlankMethod::Overlay,
            PromptStyle::Auto,
        ),
    );
    assert_eq!(selection.idle_version, Some(4));
    assert_eq!(selection.idle_label(), "ext-idle-notify v4");
    assert!(selection.history_names().starts_with("ext-idle-notify-v4"));
}
