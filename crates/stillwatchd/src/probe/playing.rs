//! Latest `Event::Media { playing }` from an attached [`MediaWatcher`].

use std::sync::{Arc, Mutex, PoisonError};

use stillwatch_core::backend::EventSink;
use stillwatch_core::event::Event;

/// Playing MPRIS player names, updated by [`MediaWatcher::watch`].
///
/// [`MediaWatcher::watch`]: stillwatch_core::backend::MediaWatcher::watch
#[derive(Clone, Default)]
pub struct Playing(Arc<Mutex<Vec<String>>>);

impl Playing {
    /// The last playing list, or empty before the first event.
    #[must_use]
    pub fn snapshot(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// A sink that stores `Event::Media { playing }`.
    #[must_use]
    pub fn sink(&self) -> Arc<dyn EventSink> {
        let inner = Arc::clone(&self.0);
        Arc::new(move |event| {
            if let Event::Media { playing } = event {
                *inner.lock().unwrap_or_else(PoisonError::into_inner) = playing;
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use stillwatch_core::event::ActivityEvent;

    use super::*;

    #[test]
    fn media_events_replace_the_list() {
        let playing = Playing::default();
        assert_eq!(playing.snapshot(), Vec::<String>::new());
        playing.sink().send(Event::Media {
            playing: vec!["mpv".into()],
        });
        assert_eq!(playing.snapshot(), ["mpv"]);
        playing.sink().send(ActivityEvent::InputIdle.into());
        assert_eq!(playing.snapshot(), ["mpv"]);
        playing.sink().send(Event::Media {
            playing: Vec::new(),
        });
        assert_eq!(playing.snapshot(), [] as [String; 0]);
    }
}
