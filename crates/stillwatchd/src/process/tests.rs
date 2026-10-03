use std::time::{Duration, Instant};

use stillwatch_core::backend::BackendError;
use stillwatch_core::mocks::now_or_never;

use super::scripted::ScriptedRunner;
use super::*;

const SECOND: Duration = Duration::from_secs(1);

fn sh(script: &str) -> CommandSpec {
    CommandSpec::new("sh", 5 * SECOND).arg("-c").arg(script)
}

#[tokio::test]
async fn captures_stdout_and_stderr_on_success() {
    let output = TokioRunner
        .run(&sh("echo out; echo err >&2"))
        .await
        .unwrap();
    assert_eq!(output.stdout, "out\n");
    assert_eq!(output.stderr, "err\n");
}

#[tokio::test]
async fn sets_and_removes_environment_variables() {
    let spec = sh(r#"printf '%s|%s' "$STILLWATCH_SET" "${HOME-unset}""#)
        .env("STILLWATCH_SET", "yes")
        .env_remove("HOME");
    let output = TokioRunner.run(&spec).await.unwrap();
    assert_eq!(output.stdout, "yes|unset");
}

#[tokio::test]
async fn stdin_is_empty() {
    let output = TokioRunner.run(&sh("cat; echo done")).await.unwrap();
    assert_eq!(output.stdout, "done\n");
}

#[tokio::test]
async fn non_zero_exit_keeps_the_trimmed_stderr() {
    let error = TokioRunner
        .run(&sh("echo '  no outputs  ' >&2; exit 3"))
        .await
        .unwrap_err();
    assert_eq!(
        error,
        CommandError::Failed {
            program: "sh".into(),
            code: Some(3),
            stderr: "no outputs".into(),
        }
    );
    assert_eq!(error.to_string(), "sh exited with status 3: no outputs");
}

#[tokio::test]
async fn a_signal_is_a_failure_without_a_code() {
    let error = TokioRunner.run(&sh("kill -9 $$")).await.unwrap_err();
    assert!(
        matches!(&error, CommandError::Failed { code: None, stderr, .. } if stderr.is_empty()),
        "{error:?}"
    );
    assert_eq!(error.to_string(), "sh was killed by a signal");
}

#[tokio::test]
async fn a_missing_program_is_not_found() {
    let spec = CommandSpec::new("stillwatch-no-such-program", SECOND);
    let error = TokioRunner.run(&spec).await.unwrap_err();
    assert_eq!(
        error,
        CommandError::NotFound {
            program: "stillwatch-no-such-program".into()
        }
    );
}

#[tokio::test]
async fn a_program_we_cant_execute_is_permission_denied() {
    let error = TokioRunner
        .run(&CommandSpec::new("/", SECOND))
        .await
        .unwrap_err();
    assert_eq!(
        error,
        CommandError::PermissionDenied {
            program: "/".into()
        }
    );
}

#[tokio::test]
async fn a_slow_program_is_killed_at_the_timeout() {
    let spec = CommandSpec::new("sleep", Duration::from_millis(50)).arg("30");
    let started = Instant::now();
    let error = TokioRunner.run(&spec).await.unwrap_err();
    assert!(started.elapsed() < 10 * SECOND);
    assert_eq!(
        error,
        CommandError::TimedOut {
            program: "sleep".into(),
            after: Duration::from_millis(50),
        }
    );
    assert_eq!(error.to_string(), "sleep timed out after 50ms");
}

#[test]
fn errors_map_onto_backend_errors() {
    let program = || "kscreen-doctor".to_owned();
    let cases = [
        (
            CommandError::NotFound { program: program() },
            BackendError::Unavailable("kscreen-doctor not found".into()),
        ),
        (
            CommandError::PermissionDenied { program: program() },
            BackendError::PermissionDenied("kscreen-doctor can't be run: permission denied".into()),
        ),
        (
            CommandError::Spawn {
                program: program(),
                message: "out of memory".into(),
            },
            BackendError::Io("kscreen-doctor can't be run: out of memory".into()),
        ),
        (
            CommandError::Failed {
                program: program(),
                code: Some(1),
                stderr: String::new(),
            },
            BackendError::Protocol("kscreen-doctor exited with status 1".into()),
        ),
        (
            CommandError::TimedOut {
                program: program(),
                after: Duration::from_secs(10),
            },
            BackendError::Io("kscreen-doctor timed out after 10s".into()),
        ),
    ];
    for (error, expected) in cases {
        assert_eq!(BackendError::from(error), expected);
    }
}

#[test]
fn other_spawn_errors_keep_the_os_message() {
    let error = CommandError::spawn("x", &std::io::Error::other("boom"));
    assert_eq!(
        error,
        CommandError::Spawn {
            program: "x".into(),
            message: "boom".into()
        }
    );
}

#[test]
fn stderr_keeps_the_tail_on_a_char_boundary() {
    assert_eq!(tail_of_stderr("  short\n"), "short");
    let long = format!("{}é{}", "a".repeat(10), "b".repeat(STDERR_LIMIT - 1));
    let tail = tail_of_stderr(&long);
    assert_eq!(tail, "b".repeat(STDERR_LIMIT - 1));
}

#[test]
fn specs_build_and_display_as_a_command_line() {
    let spec = CommandSpec::new("kscreen-doctor", SECOND)
        .args(["--dpms", "off"])
        .arg("--dpms-excluded")
        .arg("DP-1")
        .env("QT_QPA_PLATFORM", "wayland")
        .env_remove("WAYLAND_SOCKET");
    assert_eq!(
        spec.to_string(),
        "kscreen-doctor --dpms off --dpms-excluded DP-1"
    );
    assert_eq!(spec.env, [("QT_QPA_PLATFORM".into(), "wayland".into())]);
    assert_eq!(spec.env_remove, ["WAYLAND_SOCKET"]);
    assert_eq!(spec.timeout, SECOND);
}

#[test]
fn the_scripted_runner_records_calls_and_plays_results() {
    let runner = ScriptedRunner::default();
    runner.push_stderr("warning");
    runner.push_error(CommandError::NotFound {
        program: "x".into(),
    });
    let spec = CommandSpec::new("x", SECOND);

    let first = now_or_never(runner.run(&spec)).unwrap().unwrap();
    assert_eq!(first.stderr, "warning");
    assert!(now_or_never(runner.run(&spec)).unwrap().is_err());
    assert_eq!(
        now_or_never(runner.run(&spec)).unwrap(),
        Ok(CommandOutput::default())
    );
    assert_eq!(runner.calls(), [spec.clone(), spec.clone(), spec]);
}
