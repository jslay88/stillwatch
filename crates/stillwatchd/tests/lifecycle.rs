//! Spawns the real `stillwatchd` binary and drives it with signals.
//!
//! The child gets a private session bus and an empty runtime dir, so it
//! never talks to the desktop's Wayland socket or session bus.

use std::io::{self, BufRead, BufReader, Lines};
use std::process::{Child, ChildStderr, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use stillwatch_testkit::PrivateBus;

const TIMEOUT: Duration = Duration::from_secs(20);

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Daemon {
    child: Child,
    stderr: Lines<BufReader<ChildStderr>>,
    started: String,
    _bus: PrivateBus,
    _state: tempfile::TempDir,
    _runtime: tempfile::TempDir,
}

impl Daemon {
    fn spawn() -> Result<Option<Self>, Box<dyn std::error::Error>> {
        let Some(bus) = PrivateBus::start()? else {
            return Ok(None);
        };
        let state = tempfile::tempdir()?;
        let runtime = tempfile::tempdir()?;
        let mut child = Command::new(env!("CARGO_BIN_EXE_stillwatchd"))
            .args([
                "--config",
                "/nonexistent/stillwatch/config.toml",
                "--log-level",
                "info",
            ])
            .env("DBUS_SESSION_BUS_ADDRESS", bus.address())
            .env("XDG_STATE_HOME", state.path())
            .env("XDG_RUNTIME_DIR", runtime.path())
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("WAYLAND_SOCKET")
            .env_remove("JOURNAL_STREAM")
            .env_remove("RUST_LOG")
            .stderr(Stdio::piped())
            .spawn()?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("stderr wasn't piped"))?;
        let mut daemon = Self {
            child,
            stderr: BufReader::new(stderr).lines(),
            started: String::new(),
            _bus: bus,
            _state: state,
            _runtime: runtime,
        };
        daemon.started = daemon.wait_for_line("stillwatchd started")?;
        Ok(Some(daemon))
    }

    fn wait_for_line(&mut self, needle: &str) -> io::Result<String> {
        for line in self.stderr.by_ref() {
            let line = line?;
            if line.contains(needle) {
                return Ok(line);
            }
        }
        Err(io::Error::other(format!(
            "stillwatchd exited without logging {needle:?}"
        )))
    }

    fn signal(&self, name: &str) -> io::Result<()> {
        let status = Command::new("kill")
            .arg(format!("-{name}"))
            .arg(self.child.id().to_string())
            .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!("kill -{name} failed: {status}")))
        }
    }

    fn wait(mut self) -> io::Result<ExitStatus> {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            if let Some(status) = self.child.try_wait()? {
                return Ok(status);
            }
            if Instant::now() > deadline {
                self.child.kill()?;
                return Err(io::Error::other(format!(
                    "stillwatchd didn't exit within {TIMEOUT:?}"
                )));
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[test]
fn startup_logs_version_and_config_path() -> TestResult {
    let Some(daemon) = Daemon::spawn()? else {
        return Ok(());
    };
    let started = daemon.started.clone();
    assert!(started.contains(" INFO "), "{started}");
    assert!(started.contains(env!("CARGO_PKG_VERSION")), "{started}");
    assert!(
        started.contains("/nonexistent/stillwatch/config.toml"),
        "{started}"
    );
    assert!(started.contains("config_exists=false"), "{started}");
    daemon.signal("TERM")?;
    assert!(daemon.wait()?.success());
    Ok(())
}

#[test]
fn sigterm_exits_zero() -> TestResult {
    let Some(mut daemon) = Daemon::spawn()? else {
        return Ok(());
    };
    daemon.signal("TERM")?;
    let stopping = daemon.wait_for_line("stillwatchd stopping")?;
    assert!(stopping.contains("Terminate"), "{stopping}");
    assert_eq!(daemon.wait()?.code(), Some(0));
    Ok(())
}

#[test]
fn sigint_exits_zero() -> TestResult {
    let Some(daemon) = Daemon::spawn()? else {
        return Ok(());
    };
    daemon.signal("INT")?;
    assert_eq!(daemon.wait()?.code(), Some(0));
    Ok(())
}

#[test]
fn sighup_keeps_running_until_sigterm() -> TestResult {
    let Some(mut daemon) = Daemon::spawn()? else {
        return Ok(());
    };
    daemon.signal("HUP")?;
    daemon.wait_for_line("SIGHUP")?;
    assert!(daemon.child.try_wait()?.is_none());
    daemon.signal("TERM")?;
    assert_eq!(daemon.wait()?.code(), Some(0));
    Ok(())
}

#[test]
fn version_flag_prints_version() -> TestResult {
    let output = Command::new(env!("CARGO_BIN_EXE_stillwatchd"))
        .arg("--version")
        .output()?;
    assert!(output.status.success());
    let version = String::from_utf8(output.stdout)?;
    assert!(version.contains(env!("CARGO_PKG_VERSION")), "{version}");
    Ok(())
}

#[test]
fn bad_log_level_exits_with_usage_error() -> TestResult {
    let output = Command::new(env!("CARGO_BIN_EXE_stillwatchd"))
        .args(["--log-level", "loud"])
        .output()?;
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("loud"));
    Ok(())
}
