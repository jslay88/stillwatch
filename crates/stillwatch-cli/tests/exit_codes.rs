//! Runs the real `stillwatch` binary to check exit codes and messages. Daemon
//! commands always get `--bus-address` for a private bus, and the session bus
//! address is removed from their environment.

mod support;

use std::process::Output;

use stillwatch_testkit::PrivateBus;
use support::Daemon;
use tokio::process::Command;

type TestResult = Result<(), Box<dyn std::error::Error>>;

async fn stillwatch(args: &[&str]) -> std::io::Result<Output> {
    Command::new(env!("CARGO_BIN_EXE_stillwatch"))
        .args(args)
        .env_remove("RUST_LOG")
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .env("NO_COLOR", "1")
        .output()
        .await
}

async fn on_bus(address: &str, args: &[&str]) -> std::io::Result<Output> {
    let mut full = vec!["--bus-address", address];
    full.extend_from_slice(args);
    stillwatch(&full).await
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[tokio::test]
async fn daemon_not_running_exits_3() -> TestResult {
    let Some(bus) = PrivateBus::start()? else {
        return Ok(());
    };
    let output = on_bus(bus.address(), &["status"]).await?;
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(
        stderr(&output),
        "error: stillwatchd is not running; start it with systemctl --user start stillwatch\n"
    );
    assert_eq!(stdout(&output), "");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn success_exits_0() -> TestResult {
    let Some(daemon) = Daemon::start().await? else {
        return Ok(());
    };
    let output = on_bus(daemon.address(), &["pause"]).await?;
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), "paused\n");
    assert_eq!(daemon.fake.state().controls.len(), 1);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refused_snooze_exits_1_with_the_reason() -> TestResult {
    let Some(daemon) = Daemon::start().await? else {
        return Ok(());
    };
    let output = on_bus(daemon.address(), &["snooze", "13h"]).await?;
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        stderr(&output),
        "error: snooze must be at most 720 minutes\n"
    );
    assert_eq!(daemon.fake.state().controls.len(), 0);
    Ok(())
}

#[tokio::test]
async fn an_unreachable_bus_exits_1() -> TestResult {
    let dir = tempfile::tempdir()?;
    let address = format!("unix:path={}", dir.path().join("no-bus").display());
    let output = on_bus(&address, &["status"]).await?;
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).starts_with("error: can't connect to the D-Bus session bus: "),
        "{}",
        stderr(&output)
    );
    Ok(())
}

#[tokio::test]
async fn invalid_duration_is_a_usage_error() -> TestResult {
    let output = stillwatch(&["snooze", "soon"]).await?;
    assert_eq!(output.status.code(), Some(2));
    let stderr = stderr(&output);
    assert!(stderr.contains("soon"), "{stderr}");
    assert!(stderr.contains("try 45m"), "{stderr}");
    Ok(())
}

#[tokio::test]
async fn help_succeeds() -> TestResult {
    let output = stillwatch(&["--help"]).await?;
    assert!(output.status.success());
    assert!(stdout(&output).contains("idle-test"));
    Ok(())
}
