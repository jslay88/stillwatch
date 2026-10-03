//! Long-running backend watches. The loop supervises them; the action
//! runner only issues blank and unblank requests on the same instances.

use std::future::Future;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use stillwatch_core::activity::{ActivityOutput, ActivitySettings};
use stillwatch_core::backend::{
    BackendError, Blanker, EventSink, GamepadSource, IdleSource, MediaWatcher, SessionMonitor,
};
use stillwatch_core::backoff::{Backoff, BackoffPolicy};
use stillwatch_core::event::Event;
use stillwatch_core::time::Clock;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use super::inbox::Incoming;
use crate::activity::{self, ActivitySources};
use crate::config_watch::ConfigWatcher;
use crate::supervise::supervise;

/// Spawns [`activity::run`](crate::activity::run) and returns its task.
pub(super) fn spawn_activity(
    idle: Arc<dyn IdleSource>,
    gamepad: Option<Arc<dyn GamepadSource>>,
    settings: watch::Receiver<ActivitySettings>,
    clock: Arc<dyn Clock>,
    out: mpsc::UnboundedSender<Incoming>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let result = activity::run(
            ActivitySources {
                idle: idle.as_ref(),
                gamepad: gamepad.as_deref(),
            },
            settings,
            backoff(),
            clock.as_ref(),
            |output| emit_activity(&out, output),
        )
        .await;
        if let Err(error) = result {
            tracing::error!(%error, "input idle stopped");
        }
    })
}

fn emit_activity(out: &mpsc::UnboundedSender<Incoming>, output: ActivityOutput) {
    match output {
        ActivityOutput::Machine(event) => {
            let _ = out.send(Incoming::Event(Event::from(event)));
        }
        ActivityOutput::Changed(presence) => {
            tracing::debug!(?presence, "activity changed");
        }
        ActivityOutput::Timer(_) => {}
    }
}

/// Session, media, and the three blanker watches.
pub(super) fn spawn_sources(
    session: Arc<dyn SessionMonitor>,
    media: Arc<dyn MediaWatcher>,
    dpms: Arc<dyn Blanker>,
    overlay: Arc<dyn Blanker>,
    ddc: Arc<dyn Blanker>,
    clock: Arc<dyn Clock>,
    out: mpsc::UnboundedSender<Incoming>,
) -> Vec<JoinHandle<()>> {
    let sink = event_sink(out);
    vec![
        spawn_supervised("session", Arc::clone(&clock), {
            let sink = Arc::clone(&sink);
            move || {
                owned(
                    Arc::clone(&session),
                    Arc::clone(&sink),
                    SessionMonitor::watch,
                )
            }
        }),
        spawn_supervised("media", Arc::clone(&clock), {
            let sink = Arc::clone(&sink);
            move || owned(Arc::clone(&media), Arc::clone(&sink), MediaWatcher::watch)
        }),
        spawn_supervised("dpms", Arc::clone(&clock), {
            let sink = Arc::clone(&sink);
            move || owned(Arc::clone(&dpms), Arc::clone(&sink), Blanker::watch)
        }),
        spawn_supervised("overlay", Arc::clone(&clock), {
            let sink = Arc::clone(&sink);
            move || owned(Arc::clone(&overlay), Arc::clone(&sink), Blanker::watch)
        }),
        spawn_supervised("ddc", clock, {
            let sink = Arc::clone(&sink);
            move || owned(Arc::clone(&ddc), Arc::clone(&sink), Blanker::watch)
        }),
    ]
}

/// Owns `backend` inside the future so `watch`'s borrow stays inside the task.
async fn owned<T, F>(
    backend: Arc<T>,
    sink: Arc<dyn EventSink>,
    watch: F,
) -> Result<(), BackendError>
where
    T: ?Sized + Send + Sync + 'static,
    F: Fn(&T, Arc<dyn EventSink>) -> stillwatch_core::backend::BackendFuture<'_, ()>
        + Send
        + Sync
        + 'static,
{
    watch(backend.as_ref(), sink).await
}

fn spawn_supervised<F, Fut>(name: &'static str, clock: Arc<dyn Clock>, attempt: F) -> JoinHandle<()>
where
    F: FnMut() -> Fut + Send + 'static,
    Fut: Future<Output = Result<(), BackendError>> + Send + 'static,
{
    tokio::spawn(async move {
        let result = supervise(name, backoff(), clock.as_ref(), attempt).await;
        if let Err(error) = result {
            tracing::error!(backend = name, %error, "backend stopped");
        }
    })
}

fn event_sink(out: mpsc::UnboundedSender<Incoming>) -> Arc<dyn EventSink> {
    Arc::new(move |event: Event| {
        let _ = out.send(Incoming::Event(event));
    })
}

fn backoff() -> Backoff {
    Backoff::new(BackoffPolicy::default())
}

/// `wl_output` hotplug. Each attempt gets a new epoch so a compositor
/// restart cannot reuse the previous connection's generations.
pub(super) fn spawn_hotplug(
    clock: Arc<dyn Clock>,
    out: mpsc::UnboundedSender<Incoming>,
) -> JoinHandle<()> {
    let epoch = Arc::new(AtomicU64::new(0));
    spawn_supervised("outputs", clock, move || {
        let generation_epoch = epoch.fetch_add(1, Ordering::Relaxed);
        let sink = event_sink(out.clone());
        async move { crate::hotplug::watch(generation_epoch, sink).await }
    })
}

/// Config directory watcher. A failed start is logged; SIGHUP and `Reload()`
/// still work. The yielded trigger is always a file-changed reload.
pub(super) fn spawn_config_watcher(
    path: &Path,
    out: mpsc::UnboundedSender<Incoming>,
) -> Option<JoinHandle<()>> {
    let watcher = match ConfigWatcher::spawn(path) {
        Ok(watcher) => watcher,
        Err(error) => {
            tracing::warn!(%error, "config file watcher is off");
            return None;
        }
    };
    Some(tokio::spawn(async move {
        let mut watcher = watcher;
        while let Some(trigger) = watcher.next().await {
            if out.send(Incoming::Reload(trigger, None)).is_err() {
                break;
            }
        }
    }))
}
