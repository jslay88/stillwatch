//! Runs the real `stillwatch idle-test` against a missing compositor, so it
//! sits retrying the Wayland connection until it's stopped.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn idle_test(config_home: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_stillwatch"));
    command
        .args(["idle-test", "--minutes", "1"])
        .env("XDG_CONFIG_HOME", config_home)
        .env("WAYLAND_DISPLAY", "stillwatch-test-no-such-socket")
        .env_remove("WAYLAND_SOCKET")
        .env_remove("RUST_LOG");
    command
}

#[test]
fn ctrl_c_exits_cleanly() -> TestResult {
    let home = tempfile::tempdir()?;
    let mut child = idle_test(home.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let mut header = String::new();
    BufReader::new(stdout).read_line(&mut header)?;
    assert!(header.contains("with a 1m idle timeout"), "{header}");

    let pid = child.id().to_string();
    assert!(
        Command::new("kill")
            .args(["-INT", &pid])
            .status()?
            .success()
    );
    assert_eq!(child.wait()?.code(), Some(0));
    Ok(())
}

#[test]
fn a_broken_config_fails_before_watching() -> TestResult {
    let home = tempfile::tempdir()?;
    fs::create_dir(home.path().join("stillwatch"))?;
    fs::write(
        home.path().join("stillwatch/config.toml"),
        "[idle]\ninput_idle_minutes = 0\n",
    )?;
    let output = idle_test(home.path()).output()?;
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("can't load the config"), "{stderr}");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
    Ok(())
}
