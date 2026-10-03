use std::sync::Arc;

use super::{BackendFuture, EventSink};

/// MPRIS media players on the session bus.
///
/// Player names are the bus name without the `org.mpris.MediaPlayer2.` prefix
/// (for example `spotify` or `firefox.instance_1_42`), so they can be matched
/// against `media_ignore_players`.
pub trait MediaWatcher: Send + Sync {
    /// Emits `Event::Media { playing }` with every player whose
    /// `PlaybackStatus` is `Playing`: once on start, then whenever that set
    /// changes (including players appearing or vanishing).
    fn watch(&self, sink: Arc<dyn EventSink>) -> BackendFuture<'_, ()>;

    /// Every player currently on the bus, playing or not.
    fn players(&self) -> BackendFuture<'_, Vec<String>>;
}
