//! `WaylandIdleSource` against whatever compositor `WAYLAND_DISPLAY` points
//! at (the desktop, or `kwin_wayland --virtual` in CI). Without one, only the
//! "no compositor" path is checked.

use std::sync::Arc;
use std::time::Duration;

use stillwatch_core::backend::{EventSink, IdleSource};
use stillwatch_core::event::{ActivityEvent, Event};
use stillwatchd::idle::WaylandIdleSource;
use tokio::sync::mpsc;
use tokio::time::timeout;

fn compositor() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
}

fn channel_sink() -> (Arc<dyn EventSink>, mpsc::UnboundedReceiver<Event>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let sink = move |event| {
        let _ = tx.send(event);
    };
    (Arc::new(sink), rx)
}

#[tokio::test]
async fn binds_input_idle_or_reports_no_compositor() {
    let source = WaylandIdleSource::default();
    let (sink, _rx) = channel_sink();
    let watching = timeout(
        Duration::from_millis(500),
        source.watch(Duration::from_secs(600), sink),
    )
    .await;
    if compositor() {
        assert!(watching.is_err(), "watch ended early: {watching:?}");
    } else {
        let error = watching.unwrap().unwrap_err();
        assert!(error.is_transient(), "{error}");
    }
}

#[tokio::test]
async fn short_timeout_reports_input_idle() {
    if !compositor() {
        eprintln!("skipping: WAYLAND_DISPLAY is unset");
        return;
    }
    let source = WaylandIdleSource::default();
    let (sink, mut rx) = channel_sink();
    let watching = source.watch(Duration::from_millis(50), sink);
    let first = timeout(Duration::from_secs(10), async {
        tokio::select! {
            result = watching => panic!("watch ended: {result:?}"),
            event = rx.recv() => event,
        }
    })
    .await
    .unwrap();
    assert_eq!(first, Some(ActivityEvent::InputIdle.into()));
}
