//! `stillwatchd --capture-check` exits with the capture error when there's
//! no `KWin` to ask.

use std::process::Command;

#[test]
fn capture_check_without_a_session_bus_fails_clearly() {
    let output = Command::new(env!("CARGO_BIN_EXE_stillwatchd"))
        .args(["--capture-check", "HDMI-A-1"])
        .env(
            "DBUS_SESSION_BUS_ADDRESS",
            "unix:path=/nonexistent/stillwatch-bus",
        )
        .env_remove("JOURNAL_STREAM")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unavailable: no session bus"), "{stderr}");
}
