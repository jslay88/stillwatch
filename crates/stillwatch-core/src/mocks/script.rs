use std::collections::VecDeque;
use std::future::Future;
use std::pin::pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use crate::backend::{BackendError, BackendFuture, EventSink};
use crate::event::Event;
use crate::sync::lock;

/// Polls `future` once and returns its output if it was ready.
///
/// Mock futures are ready immediately unless scripted to hang, so this runs
/// them without an async runtime.
pub fn now_or_never<F: Future>(future: F) -> Option<F::Output> {
    let mut cx = Context::from_waker(Waker::noop());
    match pin!(future).poll(&mut cx) {
        Poll::Ready(output) => Some(output),
        Poll::Pending => None,
    }
}

/// A thread-safe, append-only record of calls.
#[derive(Debug)]
pub struct CallLog<T> {
    calls: Mutex<Vec<T>>,
}

impl<T> CallLog<T> {
    /// An empty log.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
        }
    }

    /// Records a call.
    pub fn push(&self, call: T) {
        lock(&self.calls).push(call);
    }

    /// Removes and returns every recorded call.
    pub fn take(&self) -> Vec<T> {
        std::mem::take(&mut *lock(&self.calls))
    }

    /// Number of recorded calls.
    pub fn len(&self) -> usize {
        lock(&self.calls).len()
    }

    /// Whether nothing was recorded.
    pub fn is_empty(&self) -> bool {
        lock(&self.calls).is_empty()
    }
}

impl<T: Clone> CallLog<T> {
    /// A copy of every recorded call, oldest first.
    pub fn snapshot(&self) -> Vec<T> {
        lock(&self.calls).clone()
    }
}

impl<T> Default for CallLog<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// A FIFO of scripted results.
#[derive(Debug)]
pub struct Script<T> {
    steps: Mutex<VecDeque<Result<T, BackendError>>>,
}

impl<T> Script<T> {
    /// An empty script.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            steps: Mutex::new(VecDeque::new()),
        }
    }

    /// Queues a result.
    pub fn push(&self, step: Result<T, BackendError>) {
        lock(&self.steps).push_back(step);
    }

    /// Takes the next result, or `None` when the script ran out.
    pub fn pop(&self) -> Option<Result<T, BackendError>> {
        lock(&self.steps).pop_front()
    }

    /// Number of queued results.
    pub fn len(&self) -> usize {
        lock(&self.steps).len()
    }

    /// Whether the script ran out.
    pub fn is_empty(&self) -> bool {
        lock(&self.steps).is_empty()
    }
}

impl<T> Default for Script<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// How a scripted `watch` call ends after sending its events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchEnd {
    /// Return `Ok(())`.
    Finish,
    /// Return this error, as on a dropped connection.
    Fail(BackendError),
    /// Never return, like a healthy source with nothing more to report.
    Hang,
}

/// One scripted `watch` call: events to send, then how to end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchRun {
    /// Sent in order on the first poll.
    pub events: Vec<Event>,
    /// What happens after the events are sent.
    pub end: WatchEnd,
}

/// Adds `push_run` to a mock that keeps its script in a `script: ScriptedWatch`
/// field.
macro_rules! scripted_watch_methods {
    () => {
        /// Queues the events and ending for the next `watch` call.
        pub fn push_run(&self, events: Vec<$crate::event::Event>, end: $crate::mocks::WatchEnd) {
            self.script.push(events, end);
        }
    };
}
pub(super) use scripted_watch_methods;

/// Scripted behavior for a backend's `watch` method.
///
/// Each call to [`watch`](Self::watch) consumes the next [`WatchRun`]; with
/// the script exhausted, `watch` sends nothing and hangs.
#[derive(Debug, Default)]
pub struct ScriptedWatch {
    runs: Mutex<VecDeque<WatchRun>>,
    calls: CallLog<()>,
}

impl ScriptedWatch {
    /// An empty script.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues a run.
    pub fn push(&self, events: Vec<Event>, end: WatchEnd) {
        lock(&self.runs).push_back(WatchRun { events, end });
    }

    /// How many times `watch` was called.
    pub fn calls(&self) -> usize {
        self.calls.len()
    }

    /// Plays the next run into `sink`.
    pub fn watch(&self, sink: Arc<dyn EventSink>) -> BackendFuture<'static, ()> {
        self.calls.push(());
        let run = lock(&self.runs).pop_front().unwrap_or(WatchRun {
            events: Vec::new(),
            end: WatchEnd::Hang,
        });
        Box::pin(async move {
            for event in run.events {
                sink.send(event);
            }
            match run.end {
                WatchEnd::Finish => Ok(()),
                WatchEnd::Fail(error) => Err(error),
                WatchEnd::Hang => std::future::pending().await,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{ActivityEvent, SessionEvent};
    use crate::mocks::RecordingSink;

    #[test]
    fn now_or_never_reports_pending() {
        assert_eq!(now_or_never(async { 5 }), Some(5));
        assert_eq!(now_or_never(std::future::pending::<()>()), None);
    }

    #[test]
    fn call_log_records_in_order() {
        let log = CallLog::default();
        assert!(log.is_empty());
        log.push(1);
        log.push(2);
        assert_eq!(log.len(), 2);
        assert_eq!(log.snapshot(), vec![1, 2]);
        assert_eq!(log.take(), vec![1, 2]);
        assert!(log.is_empty());
    }

    #[test]
    fn script_is_fifo() {
        let script = Script::default();
        script.push(Ok(1));
        script.push(Err(BackendError::Io("x".into())));
        assert_eq!(script.len(), 2);
        assert_eq!(script.pop(), Some(Ok(1)));
        assert_eq!(script.pop(), Some(Err(BackendError::Io("x".into()))));
        assert!(script.is_empty());
        assert_eq!(script.pop(), None);
    }

    #[test]
    fn scripted_watch_plays_runs_then_hangs() {
        let watch = ScriptedWatch::new();
        let sink = Arc::new(RecordingSink::new());
        watch.push(vec![ActivityEvent::InputIdle.into()], WatchEnd::Finish);
        let lost = BackendError::Disconnected("gone".into());
        watch.push(
            vec![SessionEvent::Locked.into()],
            WatchEnd::Fail(lost.clone()),
        );
        watch.push(vec![SessionEvent::Unlocked.into()], WatchEnd::Hang);

        assert_eq!(now_or_never(watch.watch(sink.clone())), Some(Ok(())));
        assert_eq!(now_or_never(watch.watch(sink.clone())), Some(Err(lost)));
        assert_eq!(now_or_never(watch.watch(sink.clone())), None);
        assert_eq!(now_or_never(watch.watch(sink.clone())), None);
        assert_eq!(watch.calls(), 4);
        assert_eq!(
            sink.take(),
            vec![
                ActivityEvent::InputIdle.into(),
                SessionEvent::Locked.into(),
                SessionEvent::Unlocked.into(),
            ]
        );
    }
}
