use std::sync::Arc;

use super::{BackendFuture, EventSink};

/// Session lock state and suspend notifications (logind, `org.freedesktop.ScreenSaver`).
pub trait SessionMonitor: Send + Sync {
    /// Emits `SessionEvent::Locked` / `Unlocked` on lock changes and
    /// `PrepareForSleep` / `ResumedFromSleep` around suspend.
    ///
    /// Does not emit the initial lock state; use
    /// [`is_locked`](Self::is_locked) for that. A `watch` started after an
    /// earlier one failed may begin with the events that happened in
    /// between, relative to what the caller already learned from
    /// `is_locked` or earlier events.
    fn watch(&self, sink: Arc<dyn EventSink>) -> BackendFuture<'_, ()>;

    /// Whether the session is locked right now.
    fn is_locked(&self) -> BackendFuture<'_, bool>;

    /// Locks the session. Locking an already locked session is a no-op.
    fn lock(&self) -> BackendFuture<'_, ()>;
}
