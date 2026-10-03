use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::script::scripted_watch_methods;
use super::{CallLog, ScriptedWatch};
use crate::backend::{BackendFuture, EventSink, SessionMonitor};

/// A [`SessionMonitor`] with scripted events and a settable lock state.
///
/// `lock` records the call and sets the lock state; it doesn't emit
/// `SessionEvent::Locked` (script that if the test needs it).
#[derive(Debug, Default)]
pub struct MockSessionMonitor {
    script: ScriptedWatch,
    locked: AtomicBool,
    lock_calls: CallLog<()>,
}

impl MockSessionMonitor {
    /// An unlocked session with an empty script (`watch` hangs).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    scripted_watch_methods!();

    /// Sets what `is_locked` returns.
    pub fn set_locked(&self, locked: bool) {
        self.locked.store(locked, Ordering::SeqCst);
    }

    /// How many times `lock` was called.
    #[must_use]
    pub fn lock_count(&self) -> usize {
        self.lock_calls.len()
    }
}

impl SessionMonitor for MockSessionMonitor {
    fn watch(&self, sink: Arc<dyn EventSink>) -> BackendFuture<'_, ()> {
        self.script.watch(sink)
    }

    fn is_locked(&self) -> BackendFuture<'_, bool> {
        Box::pin(std::future::ready(Ok(self.locked.load(Ordering::SeqCst))))
    }

    fn lock(&self) -> BackendFuture<'_, ()> {
        self.lock_calls.push(());
        self.set_locked(true);
        Box::pin(std::future::ready(Ok(())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::SessionEvent;
    use crate::mocks::{RecordingSink, WatchEnd, now_or_never};

    #[test]
    fn tracks_lock_state_and_plays_events() {
        let session = MockSessionMonitor::new();
        session.push_run(
            vec![
                SessionEvent::PrepareForSleep.into(),
                SessionEvent::ResumedFromSleep.into(),
            ],
            WatchEnd::Finish,
        );
        assert_eq!(now_or_never(session.is_locked()), Some(Ok(false)));
        assert_eq!(now_or_never(session.lock()), Some(Ok(())));
        assert_eq!(now_or_never(session.is_locked()), Some(Ok(true)));
        assert_eq!(session.lock_count(), 1);
        session.set_locked(false);
        assert_eq!(now_or_never(session.is_locked()), Some(Ok(false)));

        let sink = Arc::new(RecordingSink::new());
        assert_eq!(now_or_never(session.watch(sink.clone())), Some(Ok(())));
        assert_eq!(sink.len(), 2);
    }
}
