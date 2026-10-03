use std::sync::{Arc, Mutex};

use super::ScriptedWatch;
use super::script::scripted_watch_methods;
use crate::backend::{BackendFuture, EventSink, MediaPlayer, MediaWatcher};
use crate::sync::lock;

/// A [`MediaWatcher`] with scripted events and a settable player list.
#[derive(Debug, Default)]
pub struct MockMediaWatcher {
    script: ScriptedWatch,
    players: Mutex<Vec<MediaPlayer>>,
}

impl MockMediaWatcher {
    /// A watcher with no players and an empty script (`watch` hangs).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    scripted_watch_methods!();

    /// Replaces what `players` returns.
    pub fn set_players(&self, players: Vec<MediaPlayer>) {
        *lock(&self.players) = players;
    }
}

impl MediaWatcher for MockMediaWatcher {
    fn watch(&self, sink: Arc<dyn EventSink>) -> BackendFuture<'_, ()> {
        self.script.watch(sink)
    }

    fn players(&self) -> BackendFuture<'_, Vec<MediaPlayer>> {
        Box::pin(std::future::ready(Ok(lock(&self.players).clone())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Event;
    use crate::mocks::{RecordingSink, WatchEnd, now_or_never};

    #[test]
    fn plays_media_events_and_lists_players() {
        let media = MockMediaWatcher::new();
        let playing = Event::Media {
            playing: vec!["firefox.instance_1_42".into()],
        };
        media.push_run(vec![playing.clone()], WatchEnd::Hang);
        media.set_players(vec!["firefox.instance_1_42".into(), "spotify".into()]);

        let sink = Arc::new(RecordingSink::new());
        assert_eq!(now_or_never(media.watch(sink.clone())), None);
        assert_eq!(sink.events(), vec![playing]);
        assert_eq!(
            now_or_never(media.players()),
            Some(Ok(vec!["firefox.instance_1_42".into(), "spotify".into()]))
        );
    }
}
