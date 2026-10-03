use std::process::Stdio;

use stillwatch_core::backend::BoxFuture;
use tokio::process::Command;

use super::{
    CommandError, CommandOutput, CommandResult, CommandRunner, CommandSpec, tail_of_stderr,
};

/// Spawns real processes with `tokio::process`.
///
/// The child's stdin is `/dev/null`. On timeout, or when the future is
/// dropped, the child is killed and tokio reaps it in the background.
#[derive(Debug, Clone, Copy, Default)]
pub struct TokioRunner;

impl CommandRunner for TokioRunner {
    fn run<'a>(&'a self, spec: &'a CommandSpec) -> BoxFuture<'a, CommandResult> {
        Box::pin(run(spec))
    }
}

async fn run(spec: &CommandSpec) -> CommandResult {
    let mut command = Command::new(&spec.program);
    command
        .args(&spec.args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for key in &spec.env_remove {
        command.env_remove(key);
    }
    for (key, value) in &spec.env {
        command.env(key, value);
    }

    tracing::debug!(command = %spec, timeout = ?spec.timeout, "running");
    let child = command
        .spawn()
        .map_err(|error| CommandError::spawn(&spec.program, &error))?;
    let output = tokio::time::timeout(spec.timeout, child.wait_with_output())
        .await
        .map_err(|_| CommandError::TimedOut {
            program: spec.program.clone(),
            after: spec.timeout,
        })?
        .map_err(|error| CommandError::spawn(&spec.program, &error))?;

    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if output.status.success() {
        Ok(CommandOutput {
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr,
        })
    } else {
        Err(CommandError::Failed {
            program: spec.program.clone(),
            code: output.status.code(),
            stderr: tail_of_stderr(&stderr),
        })
    }
}
