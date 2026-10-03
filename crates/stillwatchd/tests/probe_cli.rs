//! `stillwatchd --probe` flag checks that don't capture a frame.

use std::process::Command;

#[test]
fn probe_rejects_a_too_short_interval() {
    let output = Command::new(env!("CARGO_BIN_EXE_stillwatchd"))
        .args(["--probe", "--interval", "50ms", "--count", "1"])
        .env(
            "DBUS_SESSION_BUS_ADDRESS",
            "unix:path=/nonexistent/stillwatch-bus",
        )
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("WAYLAND_SOCKET")
        .env_remove("JOURNAL_STREAM")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("at least 100 ms"), "{stderr}");
}

#[test]
fn probe_without_a_compositor_fails_clearly() {
    let output = Command::new(env!("CARGO_BIN_EXE_stillwatchd"))
        .args(["--probe", "--interval", "1s", "--count", "1"])
        .env(
            "DBUS_SESSION_BUS_ADDRESS",
            "unix:path=/nonexistent/stillwatch-bus",
        )
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("WAYLAND_SOCKET")
        .env_remove("JOURNAL_STREAM")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("can't reach the Wayland compositor"),
        "{stderr}"
    );
}
