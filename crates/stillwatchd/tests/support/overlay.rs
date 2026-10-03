//! An `OverlayBlanker` watching a private `KWin`, with its events on a
//! channel.

use std::sync::Arc;
use std::time::Duration;

use stillwatch_core::backend::{BackendError, Blanker, EventSink};
use stillwatch_core::event::{Event, PowerKind};
use stillwatchd::overlay::OverlayBlanker;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::timeout;

use super::kwin::Kwin;

/// How long a test waits for an event.
pub const WAIT: Duration = Duration::from_secs(10);

/// A blanker whose `watch` is running.
pub struct Running {
    pub blanker: Arc<OverlayBlanker>,
    pub events: mpsc::UnboundedReceiver<Event>,
    pub watch: JoinHandle<Result<(), BackendError>>,
}

/// Starts an `OverlayBlanker` watching `kwin`.
pub fn start(kwin: &Kwin) -> Running {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing_subscriber::filter::LevelFilter::DEBUG)
        .with_test_writer()
        .try_init();
    let blanker = Arc::new(OverlayBlanker::with_connector(kwin.connector()));
    let (tx, events) = mpsc::unbounded_channel();
    let sink: Arc<dyn EventSink> = Arc::new(move |event| {
        let _ = tx.send(event);
    });
    let watch = {
        let blanker = Arc::clone(&blanker);
        tokio::spawn(async move { blanker.watch(sink).await })
    };
    Running {
        blanker,
        events,
        watch,
    }
}

/// The `DisplayPower` event for `output`.
pub fn power(output: &str, on: bool) -> Event {
    Event::DisplayPower {
        output: output.to_owned(),
        on,
        kind: PowerKind::Overlay,
    }
}

impl Running {
    pub async fn next(&mut self) -> Event {
        timeout(WAIT, self.events.recv())
            .await
            .expect("no event in time")
            .expect("event channel closed")
    }

    /// The next `n` events, sorted, since outputs report in any order.
    pub async fn next_n(&mut self, n: usize) -> Vec<Event> {
        let mut events = Vec::new();
        for _ in 0..n {
            events.push(self.next().await);
        }
        events.sort_by_key(|event| format!("{event:?}"));
        events
    }

    pub async fn quiet(&mut self) {
        let extra = timeout(Duration::from_millis(300), self.events.recv()).await;
        assert!(extra.is_err(), "unexpected event: {extra:?}");
    }
}

/// Owned connector names.
pub fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|&name| name.to_owned()).collect()
}
