//! Runs the real `stillwatch` binary to check exit codes and messages.

use std::io;
use std::process::{Command, Output};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn stillwatch(args: &[&str]) -> io::Result<Output> {
    Command::new(env!("CARGO_BIN_EXE_stillwatch"))
        .args(args)
        .env_remove("RUST_LOG")
        .output()
}

#[test]
fn unavailable_command_fails_with_message() -> TestResult {
    let output = stillwatch(&["snooze", "45m"])?;
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("`stillwatch snooze` isn't available yet"),
        "{stderr}"
    );
    Ok(())
}

#[test]
fn invalid_duration_is_a_usage_error() -> TestResult {
    let output = stillwatch(&["snooze", "soon"])?;
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("soon"), "{stderr}");
    assert!(stderr.contains("try 45m"), "{stderr}");
    Ok(())
}

#[test]
fn help_succeeds() -> TestResult {
    let output = stillwatch(&["--help"])?;
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("idle-test"));
    Ok(())
}
