//! The prompt dialog. A click is delivered by the GUI over `PromptAnswer`,
//! so a process that exits 0 is not turned into another [`PromptOutcome`]:
//! that second event can land as `Dismissed` and undo the click. Exit 1,
//! the same code `kdialog` used for a close, is [`PromptOutcome::Dismissed`].

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use stillwatch_core::backend::BackendFuture;
use stillwatch_core::prompt::{PromptOutcome, PromptRequest};
use tokio::task::AbortHandle;

use super::DialogLauncher;
use crate::process::{CommandError, CommandResult, CommandRunner, CommandSpec};

/// How long the GUI may sit after the countdown. The state machine owns the
/// real timeout and dismisses the dialog; this only bounds a stuck process.
const GRACE: std::time::Duration = std::time::Duration::from_secs(30);

/// Exit status of a GUI that closed without sending `PromptAnswer`.
const CLOSED: i32 = 1;

/// One `launch` that `dismiss` can abort.
struct Running {
    token: u64,
    abort: AbortHandle,
}

/// Spawns `stillwatch-gui prompt` through a [`CommandRunner`].
///
/// `--remaining` is the seconds left on the countdown. `--custom` opens the
/// custom duration field. Exit 0 means the GUI already called `PromptAnswer`;
/// this future then waits until [`dismiss`](DialogLauncher::dismiss) so the
/// daemon does not emit that click a second time. Exit 1 is
/// [`PromptOutcome::Dismissed`].
pub struct GuiLauncher {
    runner: Arc<dyn CommandRunner>,
    next_token: AtomicU64,
    running: Mutex<Option<Running>>,
}

impl GuiLauncher {
    /// Dialogs run by `runner` (`TokioRunner` in the daemon, a script in tests).
    #[must_use]
    pub fn new(runner: Arc<dyn CommandRunner>) -> Self {
        Self {
            runner,
            next_token: AtomicU64::new(1),
            running: Mutex::new(None),
        }
    }

    fn start(&self, request: PromptRequest, custom: bool) -> BackendFuture<'_, PromptOutcome> {
        Box::pin(async move {
            let spec = command_spec(&request, custom);
            let runner = Arc::clone(&self.runner);
            let token = self.next_token.fetch_add(1, Ordering::Relaxed);
            let task = tokio::spawn(async move { runner.run(&spec).await });
            *lock(&self.running) = Some(Running {
                token,
                abort: task.abort_handle(),
            });
            let joined = task.await;
            clear_if_current(&self.running, token);
            match joined {
                Err(error) => {
                    tracing::debug!(%error, "prompt dialog closed before it exited");
                    Ok(PromptOutcome::Dismissed)
                }
                Ok(result) => after_exit(result).await,
            }
        })
    }
}

impl DialogLauncher for GuiLauncher {
    fn launch(&self, request: PromptRequest) -> BackendFuture<'_, PromptOutcome> {
        self.start(request, false)
    }

    fn launch_custom(&self, request: PromptRequest) -> BackendFuture<'_, PromptOutcome> {
        self.start(request, true)
    }

    fn dismiss(&self) -> BackendFuture<'_, ()> {
        if let Some(running) = lock(&self.running).take() {
            running.abort.abort();
        }
        Box::pin(std::future::ready(Ok(())))
    }
}

/// The `stillwatch-gui prompt` invocation for `request`.
pub(super) fn command_spec(request: &PromptRequest, custom: bool) -> CommandSpec {
    let timeout = request.countdown.saturating_add(GRACE);
    let mut spec = CommandSpec::new(gui_program(), timeout)
        .arg("prompt")
        .arg("--remaining")
        .arg(request.countdown.as_secs().to_string());
    if custom {
        spec = spec.arg("--custom");
    }
    spec
}

/// Prefer the GUI installed next to this daemon. A systemd unit's `PATH`
/// does not always include the user's `~/.local/bin`, and `ExecStart` is
/// already an absolute path to `stillwatchd`.
fn gui_program() -> String {
    let beside = std::env::current_exe().ok().and_then(|exe| {
        let path = exe.parent()?.join("stillwatch-gui");
        path.is_file().then(|| path.to_string_lossy().into_owned())
    });
    beside.unwrap_or_else(|| "stillwatch-gui".to_owned())
}

/// Exit 0 parks. The answer is already on the bus; returning it here would
/// make `start_prompt` emit `PromptAnswered` again.
async fn after_exit(
    result: CommandResult,
) -> Result<PromptOutcome, stillwatch_core::backend::BackendError> {
    match result {
        Ok(_) => std::future::pending().await,
        Err(CommandError::Failed {
            code: Some(CLOSED), ..
        }) => Ok(PromptOutcome::Dismissed),
        Err(error) => Err(error.into()),
    }
}

fn clear_if_current(slot: &Mutex<Option<Running>>, token: u64) {
    let mut running = lock(slot);
    if running.as_ref().is_some_and(|item| item.token == token) {
        *running = None;
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests;
