//! Runs the idle and gamepad sources through an [`ActivityAggregator`].
//!
//! `stillwatch idle-test` and the daemon share this, so the wiring between
//! the sources, the aggregator, and its timer exists once. The caller only
//! sees the aggregator's machine events and presence changes.

use std::pin::pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use stillwatch_core::activity::{
    ActivityAggregator, ActivityOutput, ActivitySettings, RawActivity,
};
use stillwatch_core::backend::{BackendError, BackendFuture, EventSink, GamepadSource, IdleSource};
use stillwatch_core::backoff::Backoff;
use stillwatch_core::event::Event;
use stillwatch_core::time::{Clock, TimerQueue};
use tokio::sync::{mpsc, watch};

use crate::idle;
use crate::supervise::supervise;

/// The activity sources to combine.
#[derive(Clone, Copy)]
pub struct ActivitySources<'a> {
    /// Compositor keyboard and mouse idle.
    pub idle: &'a dyn IdleSource,
    /// Gamepads, or `None` with `activity.gamepad = false`.
    pub gamepad: Option<&'a dyn GamepadSource>,
}

/// What woke the loop.
enum Wake {
    Raw(RawActivity),
    Timer,
    Settings(Option<ActivitySettings>),
}

/// Watches `sources`, aggregates them, and hands every
/// [`ActivityOutput::Machine`] and [`ActivityOutput::Changed`] to `emit`.
///
/// The idle source runs through [`idle::run`] (reconnects, and a new watch
/// when `input_idle` changes) and the gamepad source through [`supervise`].
/// Every new idle watch reaches the aggregator as
/// [`RawActivity::WatchRestarted`] before any of its events. The
/// aggregator's timer is kept here, so `emit` never sees
/// [`ActivityOutput::Timer`].
///
/// New `settings` values apply on the fly. If the sender goes away, the last
/// value stays. A gamepad source that fails for good is logged and dropped,
/// and idle carries on with keyboard and mouse only.
///
/// `clock` must follow tokio's time (the real clock, or [`TokioClock`] under
/// paused test time), since the loop sleeps on tokio timers until the
/// deadlines it computes from `clock`.
///
/// [`TokioClock`]: crate::clock::TokioClock
///
/// # Errors
///
/// Returns the idle source's first non-transient error, such as
/// [`BackendError::Unsupported`] on an ext-idle-notify v1 compositor.
pub async fn run(
    sources: ActivitySources<'_>,
    mut settings: watch::Receiver<ActivitySettings>,
    backoff: Backoff,
    clock: &dyn Clock,
    mut emit: impl FnMut(ActivityOutput),
) -> Result<(), BackendError> {
    let (raw_tx, mut raw_rx) = mpsc::unbounded_channel();
    let initial = settings.borrow_and_update().clone();
    let (timeout_tx, timeout_rx) = watch::channel(initial.input_idle);
    let mut aggregator = ActivityAggregator::new(initial);
    let mut timers = TimerQueue::new();
    let restarting = Restarting {
        inner: sources.idle,
        raw: raw_tx.clone(),
    };
    let sink = raw_sink(raw_tx);
    let mut idle = pin!(idle::run(
        &restarting,
        timeout_rx,
        Arc::clone(&sink),
        backoff.clone(),
        clock
    ));
    let mut pads = pin!(watch_gamepads(sources.gamepad, sink, backoff, clock));
    let mut settings_open = true;
    loop {
        let wake = tokio::select! {
            result = &mut idle => return result,
            () = &mut pads => continue,
            Some(raw) = raw_rx.recv() => Wake::Raw(raw),
            () = sleep_until(timers.next_deadline()) => Wake::Timer,
            changed = settings.changed(), if settings_open => {
                Wake::Settings(changed.ok().map(|()| settings.borrow_and_update().clone()))
            }
        };
        let now = clock.now();
        let outputs = match wake {
            Wake::Raw(raw) => aggregator.handle(now, raw),
            Wake::Timer => timers
                .pop_due(now)
                .into_iter()
                .filter_map(|id| RawActivity::from_event(&Event::Timer(id)))
                .flat_map(|raw| aggregator.handle(now, raw))
                .collect(),
            Wake::Settings(Some(next)) => {
                timeout_tx.send_replace(next.input_idle);
                aggregator.apply_settings(now, next)
            }
            Wake::Settings(None) => {
                settings_open = false;
                Vec::new()
            }
        };
        for output in outputs {
            match output {
                ActivityOutput::Timer(command) => {
                    timers.apply(now, &command);
                }
                other => emit(other),
            }
        }
    }
}

/// Tells the aggregator about every new idle watch before the watch can
/// send anything, so a restart is never missed or reordered.
struct Restarting<'a> {
    inner: &'a dyn IdleSource,
    raw: mpsc::UnboundedSender<RawActivity>,
}

impl IdleSource for Restarting<'_> {
    fn watch(&self, timeout: Duration, sink: Arc<dyn EventSink>) -> BackendFuture<'_, ()> {
        let _ = self.raw.send(RawActivity::WatchRestarted);
        self.inner.watch(timeout, sink)
    }
}

fn raw_sink(raw: mpsc::UnboundedSender<RawActivity>) -> Arc<dyn EventSink> {
    Arc::new(move |event: Event| {
        if let Some(activity) = RawActivity::from_event(&event) {
            let _ = raw.send(activity);
        }
    })
}

/// Supervises the gamepad source, then waits forever once it's gone.
async fn watch_gamepads(
    source: Option<&dyn GamepadSource>,
    sink: Arc<dyn EventSink>,
    backoff: Backoff,
    clock: &dyn Clock,
) {
    if let Some(source) = source {
        let result = supervise("gamepad", backoff, clock, || {
            source.watch(Arc::clone(&sink))
        })
        .await;
        if let Err(error) = result {
            tracing::warn!(%error, "gamepad activity is off until restart");
        }
    }
    std::future::pending::<()>().await;
}

async fn sleep_until(deadline: Option<Instant>) {
    match deadline {
        Some(at) => tokio::time::sleep_until(at.into()).await,
        None => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests;
