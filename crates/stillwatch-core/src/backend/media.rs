use std::sync::Arc;

use super::{BackendFuture, EventSink};

/// An MPRIS player: the bus-name suffix and the player's `Identity`.
///
/// `name` is what `media_ignore_players` suffix entries match. `identity` is
/// the `Identity` property, matched as a whole string. The two are not
/// combined into one label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaPlayer {
    /// Bus name without `org.mpris.MediaPlayer2.` (`spotify`, `firefox.instance_1_42`).
    pub name: String,
    /// `Identity` from `org.mpris.MediaPlayer2`, read once per player.
    /// Empty when the player did not answer.
    pub identity: String,
}

impl MediaPlayer {
    /// A player known only by its bus-name suffix.
    #[must_use]
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            identity: String::new(),
        }
    }

    /// A player with both the bus-name suffix and its `Identity`.
    #[must_use]
    pub fn with_identity(name: impl Into<String>, identity: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            identity: identity.into(),
        }
    }
}

impl From<&str> for MediaPlayer {
    fn from(name: &str) -> Self {
        Self::named(name)
    }
}

impl From<String> for MediaPlayer {
    fn from(name: String) -> Self {
        Self::named(name)
    }
}

/// MPRIS media players on the session bus.
///
/// Player names are the bus name without the `org.mpris.MediaPlayer2.` prefix
/// (for example `spotify` or `firefox.instance_1_42`). `Identity` is carried
/// alongside that name so `media_ignore_players` can match either.
pub trait MediaWatcher: Send + Sync {
    /// Emits `Event::Media { playing }` with every player whose
    /// `PlaybackStatus` is `Playing`: once on start, then whenever that set
    /// changes (including players appearing or vanishing).
    fn watch(&self, sink: Arc<dyn EventSink>) -> BackendFuture<'_, ()>;

    /// Every player currently on the bus, playing or not.
    fn players(&self) -> BackendFuture<'_, Vec<MediaPlayer>>;
}
