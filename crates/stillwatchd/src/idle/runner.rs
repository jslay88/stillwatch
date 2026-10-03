//! Runs an [`IdleSource`] for the life of the daemon.

use std::pin::pin;
use std::sync::Arc;
use std::time::Duration;

use stillwatch_core::backend::{BackendError, EventSink, IdleSource};
use stillwatch_core::backoff::Backoff;
use stillwatch_core::time::Clock;
use tokio::sync::watch;

use crate::supervise::supervise;

/// Watches `source` with the latest value of `timeout`, reconnecting through
/// [`supervise`] when the connection drops.
///
/// When `timeout` changes (an `idle.input_idle_minutes` reload), the current
/// watch is dropped and a new one starts with the new value, which recreates
/// the notification. Per the protocol, its idle timer starts over from
/// creation. If the `timeout` sender goes away, the last value stays in use.
///
/// # Errors
///
/// Returns the source's first non-transient error, such as
/// [`BackendError::Unsupported`] on an ext-idle-notify v1 compositor.
pub async fn run(
    source: &dyn IdleSource,
    mut timeout: watch::Receiver<Duration>,
    sink: Arc<dyn EventSink>,
    backoff: Backoff,
    clock: &dyn Clock,
) -> Result<(), BackendError> {
    loop {
        let current = *timeout.borrow_and_update();
        let mut watching = pin!(supervise("idle", backoff.clone(), clock, || {
            source.watch(current, Arc::clone(&sink))
        }));
        loop {
            tokio::select! {
                result = &mut watching => return result,
                changed = timeout.changed() => {
                    if changed.is_err() {
                        return watching.await;
                    }
                    if *timeout.borrow_and_update() != current {
                        break;
                    }
                }
            }
        }
        tracing::info!(
            old = ?current,
            new = ?*timeout.borrow(),
            "input idle timeout changed, recreating the notification"
        );
    }
}

#[cfg(test)]
mod tests {
    use stillwatch_core::backoff::BackoffPolicy;
    use stillwatch_core::event::ActivityEvent;
    use stillwatch_core::mocks::{MockIdleSource, RecordingSink, WatchEnd};
    use stillwatch_core::time::FakeClock;

    use super::*;

    const TEN_MINUTES: Duration = Duration::from_secs(600);

    fn lost() -> WatchEnd {
        WatchEnd::Fail(BackendError::Disconnected("compositor restarted".into()))
    }

    fn backoff() -> Backoff {
        Backoff::new(BackoffPolicy {
            initial: Duration::from_secs(1),
            ..BackoffPolicy::default()
        })
    }

    async fn settle() {
        tokio::time::sleep(Duration::from_secs(30)).await;
    }

    #[tokio::test(start_paused = true)]
    async fn reconnects_after_a_drop_and_keeps_forwarding_events() {
        let idle = Arc::new(MockIdleSource::new());
        idle.push_run(vec![ActivityEvent::InputIdle.into()], lost());
        idle.push_run(vec![ActivityEvent::InputResumed.into()], WatchEnd::Hang);
        let sink = Arc::new(RecordingSink::new());
        let (_tx, rx) = watch::channel(TEN_MINUTES);

        let task = {
            let (idle, sink) = (Arc::clone(&idle), Arc::clone(&sink));
            tokio::spawn(async move { run(&*idle, rx, sink, backoff(), &FakeClock::new()).await })
        };
        settle().await;
        assert_eq!(idle.timeouts(), [TEN_MINUTES, TEN_MINUTES]);
        assert_eq!(
            sink.events(),
            [
                ActivityEvent::InputIdle.into(),
                ActivityEvent::InputResumed.into()
            ]
        );
        assert!(!task.is_finished());
        task.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn a_new_timeout_recreates_the_watch() {
        let idle = Arc::new(MockIdleSource::new());
        let (tx, rx) = watch::channel(TEN_MINUTES);
        let task = {
            let idle = Arc::clone(&idle);
            tokio::spawn(async move {
                let sink = Arc::new(RecordingSink::new());
                run(&*idle, rx, sink, backoff(), &FakeClock::new()).await
            })
        };
        settle().await;
        tx.send_replace(TEN_MINUTES);
        settle().await;
        assert_eq!(idle.timeouts(), [TEN_MINUTES]);

        let five = Duration::from_secs(300);
        tx.send_replace(five);
        settle().await;
        assert_eq!(idle.timeouts(), [TEN_MINUTES, five]);

        drop(tx);
        settle().await;
        assert!(!task.is_finished());
        assert_eq!(idle.timeouts(), [TEN_MINUTES, five]);
        task.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn the_last_timeout_stays_when_the_sender_is_gone() {
        let idle = MockIdleSource::new();
        idle.push_run(Vec::new(), WatchEnd::Finish);
        let (tx, rx) = watch::channel(TEN_MINUTES);
        drop(tx);
        let sink = Arc::new(RecordingSink::new());
        let result = run(&idle, rx, sink, backoff(), &FakeClock::new()).await;
        assert_eq!(result, Ok(()));
        assert_eq!(idle.timeouts(), [TEN_MINUTES]);
    }

    #[tokio::test(start_paused = true)]
    async fn unsupported_compositor_stops_without_retrying() {
        let idle = MockIdleSource::new();
        let v1 = BackendError::Unsupported(super::super::V1_ONLY.into());
        idle.push_run(Vec::new(), WatchEnd::Fail(v1.clone()));
        let (_tx, rx) = watch::channel(TEN_MINUTES);
        let sink = Arc::new(RecordingSink::new());
        let result = run(&idle, rx, sink, backoff(), &FakeClock::new()).await;
        assert_eq!(result, Err(v1));
        assert_eq!(idle.timeouts().len(), 1);
    }
}
