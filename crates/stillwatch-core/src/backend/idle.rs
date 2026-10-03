use std::sync::Arc;
use std::time::Duration;

use super::{BackendFuture, EventSink};

/// Keyboard and mouse idle from the compositor.
///
/// Implementations must use *input* idle (ext-idle-notify v2
/// `get_input_idle_notification` or equivalent), which ignores idle
/// inhibitors, so a video call doesn't keep the user "active".
pub trait IdleSource: Send + Sync {
    /// Watches for input idle with the given timeout.
    ///
    /// Emits `ActivityEvent::InputIdle` once no keyboard or mouse input has
    /// arrived for `timeout`, then `ActivityEvent::InputResumed` on the next
    /// input, repeating for as long as the future runs.
    ///
    /// To change the timeout, drop the future and call `watch` again.
    /// Returns `Err(BackendError::Disconnected)` when the compositor
    /// connection drops, so the caller can reconnect, and
    /// `Err(BackendError::Unsupported)` when only ext-idle-notify v1 exists.
    fn watch(&self, timeout: Duration, sink: Arc<dyn EventSink>) -> BackendFuture<'_, ()>;
}
