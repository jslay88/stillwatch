//! Runs the real `stillwatch config init` and `config check`.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use stillwatch_core::config::Config;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Runs `stillwatch` with `$XDG_CONFIG_HOME` pointed at `config_home`.
fn stillwatch(config_home: &Path, args: &[&str]) -> std::io::Result<Output> {
    Command::new(env!("CARGO_BIN_EXE_stillwatch"))
        .args(args)
        .env("XDG_CONFIG_HOME", config_home)
        .env_remove("RUST_LOG")
        .output()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn init_writes_the_default_path_and_check_accepts_it() -> TestResult {
    let home = tempfile::tempdir()?;
    let path = home.path().join("stillwatch/config.toml");

    let output = stillwatch(home.path(), &["config", "init"])?;
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), format!("wrote {}\n", path.display()));

    let written = fs::read_to_string(&path)?;
    assert!(written.starts_with("# Stillwatch config\n"));
    assert_eq!(Config::from_toml_str(&written)?.config, Config::default());

    let output = stillwatch(home.path(), &["config", "check"])?;
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), format!("{}: ok\n", path.display()));
    Ok(())
}

#[test]
fn init_refuses_an_existing_file_unless_forced() -> TestResult {
    let home = tempfile::tempdir()?;
    let path = home.path().join("custom.toml");
    let path_arg = path.to_str().ok_or("non-UTF-8 temp path")?;
    fs::write(&path, "# hand edited\n")?;

    let output = stillwatch(home.path(), &["config", "init", path_arg])?;
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("already exists; pass --force to overwrite it"),
        "{}",
        stderr(&output)
    );
    assert_eq!(fs::read_to_string(&path)?, "# hand edited\n");

    let output = stillwatch(home.path(), &["config", "init", "--force", path_arg])?;
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let written = fs::read_to_string(&path)?;
    assert_eq!(Config::from_toml_str(&written)?.config, Config::default());
    Ok(())
}

#[test]
fn check_prints_every_problem_with_its_key() -> TestResult {
    let home = tempfile::tempdir()?;
    let path = home.path().join("bad.toml");
    let path_arg = path.to_str().ok_or("non-UTF-8 temp path")?;
    fs::write(
        &path,
        "[stale]\nstale_percent = 0\nignore_dark_below = 300\n\
         [action]\nmode = \"command\"\n",
    )?;

    let output = stillwatch(home.path(), &["config", "check", path_arg])?;
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        stdout(&output),
        "stale.stale_percent: must be between 1 and 100, got 0\n\
         stale.ignore_dark_below: must be between 0 and 255, got 300\n\
         action.command: must not be empty\n"
    );
    assert!(
        stderr(&output).contains("is invalid: 3 problems"),
        "{}",
        stderr(&output)
    );
    Ok(())
}

#[test]
fn check_refuses_a_newer_version() -> TestResult {
    let home = tempfile::tempdir()?;
    let path = home.path().join("future.toml");
    let path_arg = path.to_str().ok_or("non-UTF-8 temp path")?;
    fs::write(&path, "version = 2\n")?;

    let output = stillwatch(home.path(), &["config", "check", path_arg])?;
    assert_eq!(output.status.code(), Some(1));
    let stderr = stderr(&output);
    assert!(stderr.contains("is invalid"), "{stderr}");
    assert!(
        stderr.contains("config version 2 is newer than this build supports"),
        "{stderr}"
    );
    Ok(())
}

#[test]
fn check_without_a_file_suggests_init() -> TestResult {
    let home = tempfile::tempdir()?;
    let output = stillwatch(home.path(), &["config", "check"])?;
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("doesn't exist; `stillwatch config init` creates it"),
        "{}",
        stderr(&output)
    );
    Ok(())
}

#[test]
fn unknown_flag_is_a_usage_error() -> TestResult {
    let home = tempfile::tempdir()?;
    let output = stillwatch(home.path(), &["config", "check", "--bogus"])?;
    assert_eq!(output.status.code(), Some(2));
    Ok(())
}
