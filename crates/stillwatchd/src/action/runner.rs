//! Turns state-machine [`Command`]s into backend calls.

use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use stillwatch_core::backend::{BackendError, Blanker, Dimmer, HistorySink, SessionMonitor};
use stillwatch_core::command::{BlankMethod, Command, HookKind};
use stillwatch_core::config::{ActionMode, Config, DimMethod};
use stillwatch_core::event::Event;
use stillwatch_core::history::{HistoryEntry, HistoryKind};
use stillwatch_core::time::Clock;
use tokio::sync::Notify;

use super::brightness::BrightnessDimmer;
use super::hooks::hook_spec;
use super::targets::{is_strict_subset, resolve_outputs};
use crate::process::CommandRunner;

/// The blankers, dimmers, session, and helpers [`ActionRunner`] calls.
///
/// One [`OverlayBlanker`](crate::overlay::OverlayBlanker) should be cloned
/// into both `overlay` and `dimmer`. The daemon loop owns each blanker's
/// `watch` (under `supervise`); this runner only issues requests.
pub struct ActionBackends {
    /// `blank_method = "dpms"`.
    pub dpms: Arc<dyn Blanker>,
    /// Overlay blank, and the overlay fallback.
    pub overlay: Arc<dyn Blanker>,
    /// `blank_method = "ddc_standby"`.
    pub ddc: Arc<dyn Blanker>,
    /// Overlay dim (`dim_method = "overlay"`), same instance as `overlay`.
    pub dimmer: Arc<dyn Dimmer>,
    /// Session lock for `Command::Lock`.
    pub session: Arc<dyn SessionMonitor>,
    /// Hooks and `action.command`.
    pub commands: Arc<dyn CommandRunner>,
    /// Records `overlay_used` when a blanker fails and the overlay takes over.
    pub history: Arc<dyn HistorySink>,
    /// Wall time for those history entries.
    pub clock: Arc<dyn Clock>,
    /// KDE brightness dimmer. `None` uses [`BrightnessDimmer`].
    pub brightness: Option<Arc<dyn Dimmer>>,
}

/// Executes `Blank`, `Unblank`, `Lock`, and `RunHook`.
///
/// `dim_then_blank` dims inside the first `Blank` of an episode and waits
/// `dim_seconds`. Input makes the machine send [`Command::Unblank`], which
/// cancels that wait and lifts the dim. Re-blanks (already blanked) skip dim
/// and just run the method the machine picked.
#[derive(Clone)]
pub struct ActionRunner {
    inner: Arc<Inner>,
}

struct Inner {
    backends: ActionBackends,
    brightness: Arc<dyn Dimmer>,
    config: Mutex<Config>,
    connected: Mutex<Vec<String>>,
    blanked: Mutex<bool>,
    last_outputs: Mutex<Vec<String>>,
    last_method: Mutex<BlankMethod>,
    episode: AtomicU64,
    cancel_dim: Notify,
}

impl ActionRunner {
    /// Builds a runner for `config`. `connected` is the current connector
    /// list; empty means "every output the blanker can see".
    #[must_use]
    pub fn new(config: Config, connected: Vec<String>, backends: ActionBackends) -> Self {
        let brightness = backends
            .brightness
            .clone()
            .unwrap_or_else(|| Arc::new(BrightnessDimmer));
        let method = config.action.blank_method;
        Self {
            inner: Arc::new(Inner {
                backends,
                brightness,
                config: Mutex::new(config),
                connected: Mutex::new(connected),
                blanked: Mutex::new(false),
                last_outputs: Mutex::new(Vec::new()),
                last_method: Mutex::new(method),
                episode: AtomicU64::new(0),
                cancel_dim: Notify::new(),
            }),
        }
    }

    /// Applies a reloaded config. Does not change what is already blanked.
    pub fn apply_config(&self, config: &Config) {
        lock(&self.inner.config).clone_from(config);
    }

    /// Replaces the connected-output list used when resolving `all` / empty
    /// `monitored_outputs`.
    pub fn set_connected(&self, outputs: Vec<String>) {
        *lock(&self.inner.connected) = outputs;
    }

    /// The resolved target list for the current config and connected outputs.
    #[must_use]
    pub fn targets(&self) -> Vec<String> {
        let config = lock(&self.inner.config);
        let connected = lock(&self.inner.connected);
        resolve_outputs(
            config.action.outputs,
            &config.stale.monitored_outputs,
            &connected,
        )
    }

    /// Runs one machine command. `Blank` and `Lock` come back as
    /// [`Event::ActionCompleted`] or [`Event::ActionFailed`]. Hooks are
    /// spawned and never waited on.
    pub async fn execute(&self, command: &Command) -> Option<Event> {
        match command {
            Command::Blank { outputs, method } => Some(self.blank(outputs, *method).await),
            Command::Unblank { outputs } => {
                self.unblank(outputs).await;
                None
            }
            Command::Lock => Some(self.lock_session().await),
            Command::RunHook(kind) => {
                self.spawn_hook(*kind);
                None
            }
            _ => None,
        }
    }

    async fn lock_session(&self) -> Event {
        match self.inner.backends.session.is_locked().await {
            Ok(true) => {
                tracing::debug!("session already locked, skipping Lock");
                return Event::ActionCompleted;
            }
            Ok(false) => {}
            Err(error) => {
                tracing::warn!(%error, "could not read lock state, trying lock anyway");
            }
        }
        outcome(self.inner.backends.session.lock().await)
    }

    fn spawn_hook(&self, kind: HookKind) {
        let config = lock(&self.inner.config).clone();
        let script = config.hook_command(kind).to_owned();
        if script.trim().is_empty() {
            return;
        }
        let outputs = lock(&self.inner.last_outputs).clone();
        let method = *lock(&self.inner.last_method);
        let spec = hook_spec(&script, &outputs, method, kind);
        let runner = Arc::clone(&self.inner.backends.commands);
        tracing::info!(command = %spec, reason = kind.reason(), "running hook");
        tokio::spawn(async move {
            if let Err(error) = runner.run(&spec).await {
                tracing::warn!(command = %spec, %error, "hook failed");
            }
        });
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn outcome(result: Result<(), BackendError>) -> Event {
    match result {
        Ok(()) => Event::ActionCompleted,
        Err(error) => Event::ActionFailed { error },
    }
}

impl ActionRunner {
    async fn blank(&self, outputs: &[String], method: BlankMethod) -> Event {
        let outputs = self.effective(outputs);
        lock(&self.inner.last_outputs).clone_from(&outputs);
        *lock(&self.inner.last_method) = method;
        let episode = self.inner.episode.load(Ordering::SeqCst);
        let dim_first = {
            let config = lock(&self.inner.config);
            config.action.mode == ActionMode::DimThenBlank && !*lock(&self.inner.blanked)
        };
        if dim_first && let Err(error) = self.dim_phase(&outputs, method, episode).await {
            return Event::ActionFailed { error };
        }
        if self.stale(episode) {
            return cancelled();
        }
        let result = self.blank_with_fallback(&outputs, method).await;
        if result.is_ok() {
            *lock(&self.inner.blanked) = true;
        }
        outcome(result)
    }

    async fn dim_phase(
        &self,
        outputs: &[String],
        method: BlankMethod,
        episode: u64,
    ) -> Result<(), BackendError> {
        let (percent, wait) = {
            let action = &lock(&self.inner.config).action;
            (
                action.dim_percent,
                Duration::from_secs(u64::from(action.dim_seconds)),
            )
        };
        self.do_dim(outputs, percent).await?;
        if self.stale(episode) {
            let _ = self.inner.backends.dimmer.undim(outputs).await;
            return Err(cancelled_error());
        }
        tokio::select! {
            () = tokio::time::sleep(wait) => {}
            () = self.inner.cancel_dim.notified() => {
                let _ = self.inner.backends.dimmer.undim(outputs).await;
                return Err(cancelled_error());
            }
        }
        if self.stale(episode) {
            let _ = self.inner.backends.dimmer.undim(outputs).await;
            return Err(cancelled_error());
        }
        if method != BlankMethod::Overlay {
            let _ = self.inner.backends.dimmer.undim(outputs).await;
        }
        Ok(())
    }

    async fn do_dim(&self, outputs: &[String], percent: u32) -> Result<(), BackendError> {
        let method = lock(&self.inner.config).action.dim_method;
        if method == DimMethod::Brightness {
            match self.inner.brightness.dim(outputs, percent).await {
                Ok(()) => return Ok(()),
                Err(error) => tracing::warn!(
                    %error,
                    "brightness dim unavailable, using the overlay"
                ),
            }
        }
        self.inner.backends.dimmer.dim(outputs, percent).await
    }

    async fn blank_with_fallback(
        &self,
        outputs: &[String],
        method: BlankMethod,
    ) -> Result<(), BackendError> {
        // KWin applies DPMS to every output. A strict subset must not be sent.
        if method == BlankMethod::Dpms && self.partial_dpms(outputs) {
            tracing::warn!(
                targets = %outputs.join(","),
                "KWin DPMS blanks every output, so a partial target list is blanked with the overlay"
            );
            return self.overlay_fallback(outputs).await;
        }
        match self.blanker(method).blank(outputs).await {
            Ok(()) => Ok(()),
            Err(error) if method != BlankMethod::Overlay => {
                tracing::warn!(%error, method = method.as_str(), "blank failed, using overlay");
                self.overlay_fallback(outputs).await
            }
            Err(error) => Err(error),
        }
    }

    async fn overlay_fallback(&self, outputs: &[String]) -> Result<(), BackendError> {
        self.inner.backends.overlay.blank(outputs).await?;
        self.record_overlay_used().await;
        Ok(())
    }

    fn partial_dpms(&self, outputs: &[String]) -> bool {
        let connected = lock(&self.inner.connected);
        is_strict_subset(outputs, &connected)
    }

    async fn unblank(&self, outputs: &[String]) {
        self.inner.episode.fetch_add(1, Ordering::SeqCst);
        self.inner.cancel_dim.notify_waiters();
        *lock(&self.inner.blanked) = false;
        let outputs = self.effective(outputs);
        self.ignore("brightness", self.inner.brightness.undim(&outputs))
            .await;
        self.ignore("overlay", self.inner.backends.overlay.unblank(&outputs))
            .await;
        self.ignore("dpms", self.inner.backends.dpms.unblank(&outputs))
            .await;
        self.ignore("ddc", self.inner.backends.ddc.unblank(&outputs))
            .await;
    }

    async fn ignore(&self, backend: &str, work: impl Future<Output = Result<(), BackendError>>) {
        if let Err(error) = work.await {
            tracing::warn!(backend, %error, "unblank failed");
        }
    }

    fn blanker(&self, method: BlankMethod) -> &dyn Blanker {
        match method {
            BlankMethod::Dpms => &*self.inner.backends.dpms,
            BlankMethod::Overlay => &*self.inner.backends.overlay,
            BlankMethod::DdcStandby => &*self.inner.backends.ddc,
        }
    }

    fn effective(&self, outputs: &[String]) -> Vec<String> {
        if outputs.is_empty() {
            self.targets()
        } else {
            outputs.to_vec()
        }
    }

    fn stale(&self, episode: u64) -> bool {
        self.inner.episode.load(Ordering::SeqCst) != episode
    }

    async fn record_overlay_used(&self) {
        let entry = HistoryEntry::new(
            self.inner.backends.clock.wall_now(),
            HistoryKind::OverlayUsed,
        )
        .with_blank_method(BlankMethod::Overlay);
        if let Err(error) = self.inner.backends.history.record(entry).await {
            tracing::warn!(%error, "could not record overlay_used");
        }
    }
}

fn cancelled() -> Event {
    Event::ActionFailed {
        error: cancelled_error(),
    }
}

fn cancelled_error() -> BackendError {
    BackendError::Io("dim cancelled by Unblank (input while Acting)".into())
}
