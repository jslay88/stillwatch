//! Interim `kdialog` dialog. Answers are [`PromptOutcome`]s, the same value
//! D-Bus `PromptAnswer` already delivers.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use stillwatch_core::backend::BackendFuture;
use stillwatch_core::prompt::{PromptOutcome, PromptRequest};
use tokio::task::AbortHandle;

use super::DialogLauncher;
use super::menu::{menu_spec, outcome_of};
use crate::process::CommandRunner;

/// One `launch` that `dismiss` can abort.
struct Running {
    token: u64,
    abort: AbortHandle,
}

/// Runs `kdialog --menu` through a [`CommandRunner`].
///
/// The menu uses the notification's action keys, so a choice maps onto the
/// same [`PromptOutcome`]. Closing the dialog (exit 1) is
/// [`PromptOutcome::Dismissed`]. Dropping the launch, or [`dismiss`](DialogLauncher::dismiss),
/// aborts the process.
pub struct KdialogLauncher {
    runner: Arc<dyn CommandRunner>,
    next_token: AtomicU64,
    running: Mutex<Option<Running>>,
}

impl KdialogLauncher {
    /// Dialogs run by `runner` (`TokioRunner` in the daemon, a script in tests).
    #[must_use]
    pub fn new(runner: Arc<dyn CommandRunner>) -> Self {
        Self {
            runner,
            next_token: AtomicU64::new(1),
            running: Mutex::new(None),
        }
    }
}

impl DialogLauncher for KdialogLauncher {
    fn launch(&self, request: PromptRequest) -> BackendFuture<'_, PromptOutcome> {
        Box::pin(async move {
            let spec = menu_spec(&request);
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
                Ok(result) => outcome_of(result),
                Err(error) => {
                    tracing::debug!(%error, "dialog closed before it answered");
                    Ok(PromptOutcome::Dismissed)
                }
            }
        })
    }

    fn dismiss(&self) -> BackendFuture<'_, ()> {
        if let Some(running) = lock(&self.running).take() {
            running.abort.abort();
        }
        Box::pin(std::future::ready(Ok(())))
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
