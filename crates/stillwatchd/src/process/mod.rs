//! Running helper programs from the daemon: `kscreen-doctor`, user hooks
//! (`on_blank_cmd`, `on_resume_cmd`), and panel care's `trigger_cmd`.
//!
//! Backends describe what to run with a [`CommandSpec`] and hand it to a
//! [`CommandRunner`]. [`TokioRunner`] spawns the real process; unit tests use
//! `process::scripted::ScriptedRunner` instead, so command construction and
//! error handling are tested without spawning anything.
//!
//! Every run has a timeout, never reads stdin, and captures stdout and stderr.
//! A non-zero exit, a timeout, or a missing program is a typed
//! [`CommandError`], which converts into a [`BackendError`].

mod runner;
#[cfg(test)]
pub mod scripted;

use std::fmt;
use std::time::Duration;

use stillwatch_core::backend::{BackendError, BoxFuture};

pub use runner::TokioRunner;

/// How much of a program's stderr is kept in a [`CommandError`].
pub const STDERR_LIMIT: usize = 2048;

/// A program to run, with its arguments, environment changes, and timeout.
///
/// The program is looked up on `PATH` unless it contains a `/`. Arguments are
/// passed as-is, with no shell in between; hooks that want a shell run
/// `sh -c <command>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    /// The program name or path.
    pub program: String,
    /// Arguments, in order.
    pub args: Vec<String>,
    /// Variables set for the child, on top of the daemon's environment.
    pub env: Vec<(String, String)>,
    /// Variables removed from the child's environment.
    pub env_remove: Vec<String>,
    /// How long the program may run before it's killed.
    pub timeout: Duration,
}

impl CommandSpec {
    /// Runs `program` with no arguments, killing it after `timeout`.
    #[must_use]
    pub fn new(program: impl Into<String>, timeout: Duration) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            env: Vec::new(),
            env_remove: Vec::new(),
            timeout,
        }
    }

    /// Adds one argument.
    #[must_use]
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Adds several arguments.
    #[must_use]
    pub fn args<I, A>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = A>,
        A: Into<String>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Sets an environment variable for the child.
    #[must_use]
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    /// Removes an environment variable from the child's environment.
    #[must_use]
    pub fn env_remove(mut self, key: impl Into<String>) -> Self {
        self.env_remove.push(key.into());
        self
    }
}

impl fmt::Display for CommandSpec {
    /// The command line, for logs and error messages.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.program)?;
        for arg in &self.args {
            write!(f, " {arg}")?;
        }
        Ok(())
    }
}

/// What a successful run printed, decoded as lossy UTF-8.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommandOutput {
    /// Everything written to stdout.
    pub stdout: String,
    /// Everything written to stderr. Some tools report errors here and still
    /// exit 0, so callers may need to look at it.
    pub stderr: String,
}

/// Why a command didn't run to a successful exit.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CommandError {
    /// The program isn't installed or isn't on `PATH`.
    #[error("{program} not found")]
    NotFound {
        /// The program that was run.
        program: String,
    },
    /// The program exists but isn't executable by us.
    #[error("{program} can't be run: permission denied")]
    PermissionDenied {
        /// The program that was run.
        program: String,
    },
    /// Spawning or waiting failed for another reason.
    #[error("{program} can't be run: {message}")]
    Spawn {
        /// The program that was run.
        program: String,
        /// The operating system's error.
        message: String,
    },
    /// The program ran and failed.
    #[error("{program} {}{}", exit_text(*.code), stderr_text(.stderr))]
    Failed {
        /// The program that was run.
        program: String,
        /// The exit code, or `None` if a signal ended it.
        code: Option<i32>,
        /// The end of its stderr, trimmed, at most [`STDERR_LIMIT`] bytes.
        stderr: String,
    },
    /// The program ran past its timeout and was killed.
    #[error("{program} timed out after {after:?}")]
    TimedOut {
        /// The program that was run.
        program: String,
        /// The timeout that elapsed.
        after: Duration,
    },
}

impl CommandError {
    /// Classifies a failure to spawn `program`.
    #[must_use]
    pub fn spawn(program: &str, error: &std::io::Error) -> Self {
        let program = program.to_owned();
        match error.kind() {
            std::io::ErrorKind::NotFound => Self::NotFound { program },
            std::io::ErrorKind::PermissionDenied => Self::PermissionDenied { program },
            _ => Self::Spawn {
                program,
                message: error.to_string(),
            },
        }
    }
}

impl From<CommandError> for BackendError {
    /// A missing program means the backend is unavailable; a failed run means
    /// it answered with an error. Timeouts and other spawn errors are I/O
    /// trouble that may clear on a retry.
    fn from(error: CommandError) -> Self {
        let message = error.to_string();
        match error {
            CommandError::NotFound { .. } => Self::Unavailable(message),
            CommandError::PermissionDenied { .. } => Self::PermissionDenied(message),
            CommandError::Failed { .. } => Self::Protocol(message),
            CommandError::Spawn { .. } | CommandError::TimedOut { .. } => Self::Io(message),
        }
    }
}

/// The result of one run.
pub type CommandResult = Result<CommandOutput, CommandError>;

/// Runs a [`CommandSpec`] to completion.
///
/// Dropping the returned future kills the child.
pub trait CommandRunner: Send + Sync {
    /// Runs `spec` and waits for it, up to its timeout.
    ///
    /// Succeeds only on exit status 0.
    fn run<'a>(&'a self, spec: &'a CommandSpec) -> BoxFuture<'a, CommandResult>;
}

/// Trims `stderr` and keeps at most its last [`STDERR_LIMIT`] bytes, where
/// tools usually print the actual error.
#[must_use]
pub fn tail_of_stderr(stderr: &str) -> String {
    let trimmed = stderr.trim();
    let mut start = trimmed.len().saturating_sub(STDERR_LIMIT);
    while !trimmed.is_char_boundary(start) {
        start += 1;
    }
    trimmed[start..].to_owned()
}

fn exit_text(code: Option<i32>) -> String {
    code.map_or_else(
        || "was killed by a signal".to_owned(),
        |code| format!("exited with status {code}"),
    )
}

fn stderr_text(stderr: &str) -> String {
    if stderr.is_empty() {
        String::new()
    } else {
        format!(": {stderr}")
    }
}
#[cfg(test)]
mod tests;
