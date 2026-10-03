use std::sync::Arc;
use std::time::Duration;

use super::{CallLog, ScriptedWatch, WatchEnd};
use crate::backend::{BackendFuture, EventSink, IdleSource};
use crate::event::Event;

/// An [`IdleSource`] that plays scripted runs and records each timeout.
#[derive(Debug, Default)]
pub struct MockIdleSource {
    script: ScriptedWatch,
    timeouts: CallLog<Duration>,
}

impl MockIdleSource {
    /// A source with an empty script (`watch` hangs).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues the events and ending for the next `watch` call.
    pub fn push_run(&self, events: Vec<Event>, end: WatchEnd) {
        self.script.push(events, end);
    }

    /// The timeout passed to each `watch` call, oldest first.
    #[must_use]
    pub fn timeouts(&self) -> Vec<Duration> {
        self.timeouts.snapshot()
    }
}

impl IdleSource for MockIdleSource {
    fn watch(&self, timeout: Duration, sink: Arc<dyn EventSink>) -> BackendFuture<'_, ()> {
        self.timeouts.push(timeout);
        self.script.watch(sink)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::BackendError;
    use crate::event::ActivityEvent;
    use crate::mocks::{RecordingSink, now_or_never};

    #[test]
    fn plays_the_script_and_records_timeouts() {
        let idle = MockIdleSource::new();
        let lost = BackendError::Disconnected("wayland".into());
        idle.push_run(
            vec![
                ActivityEvent::InputIdle.into(),
                ActivityEvent::InputResumed.into(),
            ],
            WatchEnd::Fail(lost.clone()),
        );
        let source: Arc<dyn IdleSource> = Arc::new(idle);
        let sink = Arc::new(RecordingSink::new());

        let ten_minutes = Duration::from_secs(600);
        assert_eq!(
            now_or_never(source.watch(ten_minutes, sink.clone())),
            Some(Err(lost))
        );
        assert_eq!(now_or_never(source.watch(ten_minutes, sink.clone())), None);
        assert_eq!(
            sink.events(),
            vec![
                ActivityEvent::InputIdle.into(),
                ActivityEvent::InputResumed.into()
            ]
        );
    }

    #[test]
    fn remembers_each_timeout() {
        let idle = MockIdleSource::new();
        let sink = Arc::new(RecordingSink::new());
        let _ = now_or_never(idle.watch(Duration::from_secs(1), sink.clone()));
        let _ = now_or_never(idle.watch(Duration::from_secs(2), sink));
        assert_eq!(
            idle.timeouts(),
            vec![Duration::from_secs(1), Duration::from_secs(2)]
        );
    }
}
