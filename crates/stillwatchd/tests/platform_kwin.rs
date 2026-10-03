//! Platform probe against headless `KWin`. Never the desktop session, and never
//! a capture, blank, DPMS, or portal dialog.

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use stillwatch_core::command::BlankMethod;
use stillwatch_core::config::{CaptureBackend, Config};
use stillwatch_testkit::kwin::{Authorization, Kwin, KwinOptions, SCREENSHOT2};
use stillwatchd::platform::{self, CaptureChoice, ProbeEnv};

const NAME_TIMEOUT: Duration = Duration::from_secs(15);

fn env(apps: PathBuf) -> ProbeEnv {
    ProbeEnv {
        app_dirs: vec![apps],
        exe: platform::current_exe(),
        path: OsString::new(),
        i2c_dev: PathBuf::from("/nonexistent/stillwatch-i2c"),
        drm: PathBuf::from("/nonexistent/stillwatch-drm"),
        dev: PathBuf::from("/nonexistent/stillwatch-dev"),
    }
}

#[tokio::test]
async fn auto_picks_kwin_when_the_desktop_grant_is_installed() {
    let Some(kwin) = Kwin::start(KwinOptions {
        authorize: vec![Authorization::current_exe(&[SCREENSHOT2]).unwrap()],
        ..KwinOptions::default()
    })
    .await
    .unwrap() else {
        return;
    };
    kwin.wait_for_name(SCREENSHOT2, NAME_TIMEOUT).await.unwrap();

    let wayland = kwin.connect().unwrap();
    let session = kwin.bus().connect().await.unwrap();
    let granted = platform::probe_with(
        &wayland,
        Some(&session),
        None,
        &env(kwin.data_home().join("applications")),
    )
    .await
    .unwrap();

    assert!(
        granted
            .wayland
            .idle_notifier_version
            .is_some_and(|version| version >= 2),
        "{granted:?}"
    );
    assert!(granted.wayland.kwin_dpms, "{granted:?}");
    assert!(granted.wayland.layer_shell, "{granted:?}");
    assert!(!granted.wayland.wlr_capture.screencopy_manager);
    assert!(!granted.wayland.wlr_capture.image_copy_capture);
    assert!(granted.dbus.kwin.present, "{granted:?}");
    assert!(granted.dbus.kwin.screenshot2, "{granted:?}");
    assert!(
        granted.dbus.kwin.screenshot2_version.is_some(),
        "{granted:?}"
    );
    assert!(granted.dbus.kwin.screenshot2_authorized);
    assert!(!granted.dbus.portal_screencast);
    assert!(!granted.dbus.notifications);
    assert!(!granted.dbus.session.logind);
    assert!(!granted.dbus.session.screensaver);
    assert!(!granted.tools.kscreen_doctor);
    assert!(!granted.tools.i2c);

    let mut config = Config::default();
    config.capture.backend = CaptureBackend::Auto;
    let selection = platform::select(&granted, &config);
    assert!(
        selection
            .startup_failure(&env(PathBuf::new()).exe)
            .is_none()
    );
    assert!(matches!(selection.capture, CaptureChoice::Kwin { .. }));
    assert_eq!(selection.capture_name(), "kwin");
    assert!(selection.blank.fell_back);
    assert_eq!(selection.blank.method, BlankMethod::Overlay);

    let bare = platform::probe_with(
        &wayland,
        Some(&session),
        None,
        &env(kwin.data_home().join("no-such-applications")),
    )
    .await
    .unwrap();
    assert!(!bare.dbus.kwin.screenshot2_authorized);
    let idle_only = platform::select(&bare, &config);
    assert_eq!(idle_only.capture, CaptureChoice::InputIdleOnly);
}
