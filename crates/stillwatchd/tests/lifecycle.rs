//! Spawns the real `stillwatchd` binary and drives it with signals.

use std::io::{self, BufRead, BufReader, Lines};
use std::process::{Child, ChildStderr, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::from_secs(10);

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Daemon {
    child: Child,
    stderr: Lines<BufReader<ChildStderr>>,
    started: String,
}

impl Daemon {
    fn spawn() -> io::Result<Self> {
        let mut child = Command::new(env!("CARGO_BIN_EXE_stillwatchd"))
            .args([
                "--config",
                "/nonexistent/stillwatch/config.toml",
                "--log-level",
                "info",
            ])
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
        };
        daemon.started = daemon.wait_for_line("stillwatchd started")?;
        Ok(daemon)
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

#[test]
fn startup_logs_version_and_config_path() -> TestResult {
    let daemon = Daemon::spawn()?;
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
    let mut daemon = Daemon::spawn()?;
    daemon.signal("TERM")?;
    let stopping = daemon.wait_for_line("stillwatchd stopping")?;
    assert!(stopping.contains("Terminate"), "{stopping}");
    assert_eq!(daemon.wait()?.code(), Some(0));
    Ok(())
}

#[test]
fn sigint_exits_zero() -> TestResult {
    let daemon = Daemon::spawn()?;
    daemon.signal("INT")?;
    assert_eq!(daemon.wait()?.code(), Some(0));
    Ok(())
}

#[test]
fn sighup_keeps_running_until_sigterm() -> TestResult {
    let mut daemon = Daemon::spawn()?;
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
