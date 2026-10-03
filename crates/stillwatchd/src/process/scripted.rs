//! A [`CommandRunner`] for unit tests that records every spec and answers
//! from a script, without spawning anything.

use std::collections::VecDeque;
use std::sync::{Mutex, PoisonError};

use stillwatch_core::backend::BoxFuture;
use stillwatch_core::mocks::CallLog;

use super::{CommandError, CommandOutput, CommandResult, CommandRunner, CommandSpec};

/// Records each [`CommandSpec`] it's asked to run and returns the next
/// scripted result. With the script empty, runs succeed with no output.
#[derive(Debug)]
pub struct ScriptedRunner {
    calls: CallLog<CommandSpec>,
    results: Mutex<VecDeque<CommandResult>>,
}

impl ScriptedRunner {
    /// A runner where every command succeeds silently.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            calls: CallLog::new(),
            results: Mutex::new(VecDeque::new()),
        }
    }

    /// Queues the result of the next run.
    pub fn push(&self, result: CommandResult) {
        self.results
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push_back(result);
    }

    /// Queues a successful run that printed `stderr`.
    pub fn push_stderr(&self, stderr: &str) {
        self.push(Ok(CommandOutput {
            stdout: String::new(),
            stderr: stderr.to_owned(),
        }));
    }

    /// Queues a failed run.
    pub fn push_error(&self, error: CommandError) {
        self.push(Err(error));
    }

    /// Every spec run so far, oldest first.
    #[must_use]
    pub fn calls(&self) -> Vec<CommandSpec> {
        self.calls.snapshot()
    }
}

impl Default for ScriptedRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandRunner for ScriptedRunner {
    fn run<'a>(&'a self, spec: &'a CommandSpec) -> BoxFuture<'a, CommandResult> {
        self.calls.push(spec.clone());
        let result = self
            .results
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front()
            .unwrap_or_else(|| Ok(CommandOutput::default()));
        Box::pin(std::future::ready(result))
    }
}
