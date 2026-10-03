//! The harness against a real `kwin_wayland --virtual`. Skipped when it isn't
//! installed, unless `STILLWATCH_REQUIRE_KWIN=1`.

use std::collections::HashMap;
use std::io::Read as _;
use std::time::Duration;

use stillwatch_testkit::kwin::{Authorization, Kwin, KwinOptions, SCREENSHOT2};
use zbus::zvariant::{Fd, OwnedValue, Value};

const WAIT: Duration = Duration::from_secs(10);

fn version(globals: &[wayland_client::globals::Global], interface: &str) -> Option<u32> {
    globals
        .iter()
        .find(|global| global.interface == interface)
        .map(|global| global.version)
}

#[tokio::test]
async fn advertises_input_idle_v2_dpms_and_every_output() {
    let options = KwinOptions {
        outputs: 2,
        ..KwinOptions::default()
    };
    let Some(kwin) = Kwin::start(options).await.unwrap() else {
        return;
    };
    let globals = kwin.globals(WAIT).await.unwrap();
    let idle = version(&globals, "ext_idle_notifier_v1");
    assert!(idle >= Some(2), "ext_idle_notifier_v1 is {idle:?}");
    assert!(version(&globals, "org_kde_kwin_dpms_manager").is_some());
    let outputs = globals
        .iter()
        .filter(|g| g.interface == "wl_output")
        .count();
    assert_eq!(outputs, 2);
}

#[tokio::test]
async fn clients_see_only_the_sandbox() {
    let Some(kwin) = Kwin::start(KwinOptions::default()).await.unwrap() else {
        return;
    };
    let output = kwin.command("env").output().unwrap();
    assert!(output.status.success());
    let env = String::from_utf8(output.stdout).unwrap();
    let runtime = kwin.runtime_dir();
    let expected = [
        format!("WAYLAND_DISPLAY={}", kwin.wayland_display()),
        format!("XDG_RUNTIME_DIR={}", runtime.display()),
        format!("DBUS_SESSION_BUS_ADDRESS={}", kwin.dbus_address()),
    ];
    for line in &expected {
        assert!(
            env.lines().any(|l| l == line),
            "{line} missing from:\n{env}"
        );
    }
    assert!(!env.lines().any(|l| l.starts_with("DISPLAY=")), "{env}");
    assert_eq!(runtime.join(kwin.wayland_display()), kwin.socket_path());
    assert!(kwin.connect().is_ok());
}

#[tokio::test]
async fn waiting_for_an_absent_name_times_out_with_the_log() {
    let Some(kwin) = Kwin::start(KwinOptions::default()).await.unwrap() else {
        return;
    };
    let err = kwin
        .wait_for_name("org.example.Nobody", Duration::from_millis(100))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("org.example.Nobody"), "{err}");
}

type Frame = HashMap<String, OwnedValue>;

/// Calls `CaptureScreen` on the first output. A drained pipe stands in for a
/// real capture's destination.
async fn capture(kwin: &Kwin) -> Result<Frame, Box<dyn std::error::Error>> {
    kwin.wait_for_name(SCREENSHOT2, WAIT).await?;
    let conn = kwin.bus().connect().await?;
    let (read, write) = rustix::pipe::pipe()?;
    let drain = std::thread::spawn(move || {
        let mut sink = Vec::new();
        let _ = std::fs::File::from(read).read_to_end(&mut sink);
    });
    let options: HashMap<&str, Value<'_>> = HashMap::new();
    let reply = conn
        .call_method(
            Some(SCREENSHOT2),
            "/org/kde/KWin/ScreenShot2",
            Some(SCREENSHOT2),
            "CaptureScreen",
            &("Virtual-0", options, Fd::from(&write)),
        )
        .await;
    drop(write);
    let _ = drain.join();
    Ok(reply?.body().deserialize()?)
}

#[tokio::test]
async fn screenshot2_trusts_only_authorized_executables() {
    let Some(stranger) = Kwin::start(KwinOptions::default()).await.unwrap() else {
        return;
    };
    let refused = capture(&stranger).await.unwrap_err();
    assert!(refused.to_string().contains("not authorized"), "{refused}");
    drop(stranger);

    let width = 1280u32;
    let height = 720u32;
    let options = KwinOptions {
        width,
        height,
        authorize: vec![Authorization::current_exe(&[SCREENSHOT2]).unwrap()],
        ..KwinOptions::default()
    };
    let kwin = Kwin::start(options).await.unwrap().unwrap();
    let frame = capture(&kwin).await.expect("ScreenShot2 frame");
    assert_eq!(frame.get("width"), Some(&OwnedValue::from(width)));
    assert_eq!(frame.get("height"), Some(&OwnedValue::from(height)));
}
