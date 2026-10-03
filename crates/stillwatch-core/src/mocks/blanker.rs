use std::sync::Arc;

use super::{CallLog, Script, ScriptedWatch, WatchEnd};
use crate::backend::{BackendError, BackendFuture, Blanker, EventSink};
use crate::event::Event;

/// A call made to a [`MockBlanker`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlankerCall {
    /// `blank(outputs)`.
    Blank(Vec<String>),
    /// `unblank(outputs)`.
    Unblank(Vec<String>),
}

/// A [`Blanker`] that records calls, fails on demand, and plays scripted
/// power events from `watch`.
///
/// `blank` and `unblank` share one result queue; with it empty they succeed.
#[derive(Debug, Default)]
pub struct MockBlanker {
    calls: CallLog<BlankerCall>,
    results: Script<()>,
    power: ScriptedWatch,
}

impl MockBlanker {
    /// A blanker where every call succeeds.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Makes the next `blank` or `unblank` fail.
    pub fn fail_next(&self, error: BackendError) {
        self.results.push(Err(error));
    }

    /// Queues the power events and ending for the next `watch` call.
    pub fn push_power_run(&self, events: Vec<Event>, end: WatchEnd) {
        self.power.push(events, end);
    }

    /// Every `blank` and `unblank` call, oldest first.
    #[must_use]
    pub fn calls(&self) -> Vec<BlankerCall> {
        self.calls.snapshot()
    }

    fn respond(&self, call: BlankerCall) -> BackendFuture<'_, ()> {
        self.calls.push(call);
        Box::pin(std::future::ready(self.results.pop().unwrap_or(Ok(()))))
    }
}

impl Blanker for MockBlanker {
    fn blank<'a>(&'a self, outputs: &'a [String]) -> BackendFuture<'a, ()> {
        self.respond(BlankerCall::Blank(outputs.to_vec()))
    }

    fn unblank<'a>(&'a self, outputs: &'a [String]) -> BackendFuture<'a, ()> {
        self.respond(BlankerCall::Unblank(outputs.to_vec()))
    }

    fn watch(&self, sink: Arc<dyn EventSink>) -> BackendFuture<'_, ()> {
        self.power.watch(sink)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mocks::{RecordingSink, now_or_never};

    #[test]
    fn records_calls_and_fails_on_demand() {
        let blanker = MockBlanker::new();
        let outputs = vec!["HDMI-A-1".to_owned()];
        blanker.fail_next(BackendError::Unavailable("kscreen-doctor".into()));

        assert!(matches!(
            now_or_never(blanker.blank(&outputs)),
            Some(Err(BackendError::Unavailable(_)))
        ));
        assert_eq!(now_or_never(blanker.blank(&outputs)), Some(Ok(())));
        assert_eq!(now_or_never(blanker.unblank(&[])), Some(Ok(())));
        assert_eq!(
            blanker.calls(),
            vec![
                BlankerCall::Blank(outputs.clone()),
                BlankerCall::Blank(outputs),
                BlankerCall::Unblank(Vec::new()),
            ]
        );
    }

    #[test]
    fn plays_scripted_power_events() {
        let blanker = MockBlanker::new();
        let woke = Event::DisplayPower {
            output: "HDMI-A-1".into(),
            on: true,
        };
        blanker.push_power_run(vec![woke.clone()], WatchEnd::Hang);
        let sink = Arc::new(RecordingSink::new());
        assert_eq!(now_or_never(blanker.watch(sink.clone())), None);
        assert_eq!(sink.events(), vec![woke]);
    }
}
