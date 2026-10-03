//! `WaylandIdleSource` against a headless `kwin_wayland --virtual`. Nothing
//! sends input to it, so input idle fires as soon as the timeout passes.

use std::sync::Arc;
use std::time::Duration;

use stillwatch_core::backend::{BackendError, EventSink, IdleSource};
use stillwatch_core::backoff::{Backoff, BackoffPolicy};
use stillwatch_core::event::{ActivityEvent, Event};
use stillwatch_core::time::SystemClock;
use stillwatch_testkit::kwin::{Kwin, KwinOptions};
use stillwatchd::idle::{self, WaylandIdleSource};
use tokio::sync::{mpsc, watch};
use tokio::time::{Instant, timeout};

fn channel_sink() -> (Arc<dyn EventSink>, mpsc::UnboundedReceiver<Event>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let sink = move |event| {
        let _ = tx.send(event);
    };
    (Arc::new(sink), rx)
}

fn source(kwin: &Kwin) -> WaylandIdleSource {
    let connect = kwin.connector();
    WaylandIdleSource::with_connector(move || {
        connect().map_err(|err| BackendError::Disconnected(err.to_string()))
    })
}

#[tokio::test]
async fn binds_input_idle_and_keeps_watching() {
    let Some(kwin) = Kwin::start(KwinOptions::default()).await.unwrap() else {
        return;
    };
    let source = source(&kwin);
    let (sink, mut rx) = channel_sink();
    let watching = timeout(
        Duration::from_millis(500),
        source.watch(Duration::from_secs(600), sink),
    )
    .await;
    assert!(watching.is_err(), "watch ended early: {watching:?}");
    assert!(rx.try_recv().is_err(), "nothing should fire before 600 s");
}

#[tokio::test]
async fn input_idle_fires_after_the_timeout_without_input() {
    let Some(kwin) = Kwin::start(KwinOptions::default()).await.unwrap() else {
        return;
    };
    let source = source(&kwin);
    let (sink, mut rx) = channel_sink();
    let idle_after = Duration::from_secs(2);
    let started = Instant::now();
    let watching = source.watch(idle_after, sink);
    let first = timeout(Duration::from_secs(15), async {
        tokio::select! {
            result = watching => panic!("watch ended: {result:?}"),
            event = rx.recv() => event,
        }
    })
    .await
    .unwrap();
    let waited = started.elapsed();
    assert_eq!(first, Some(ActivityEvent::InputIdle.into()));
    assert!(
        waited >= Duration::from_millis(1900),
        "idle after {waited:?}"
    );
}

#[tokio::test]
async fn losing_kwin_is_a_transient_error() {
    let Some(kwin) = Kwin::start(KwinOptions::default()).await.unwrap() else {
        return;
    };
    let source = source(&kwin);
    let (sink, _rx) = channel_sink();
    let watching = source.watch(Duration::from_secs(600), sink);
    tokio::pin!(watching);
    assert!(
        timeout(Duration::from_millis(300), &mut watching)
            .await
            .is_err()
    );
    drop(kwin);
    let error = timeout(Duration::from_secs(10), watching)
        .await
        .unwrap()
        .unwrap_err();
    assert!(error.is_transient(), "{error}");
    let reconnect = source.watch(Duration::from_secs(600), channel_sink().0);
    assert!(reconnect.await.unwrap_err().is_transient());
}

#[tokio::test]
async fn reconnects_after_kwin_restarts() {
    let Some(mut kwin) = Kwin::start(KwinOptions::default()).await.unwrap() else {
        return;
    };
    let source = source(&kwin);
    let (_timeout, timeout_rx) = watch::channel(Duration::from_secs(1));
    let (sink, mut rx) = channel_sink();
    let backoff = Backoff::new(BackoffPolicy {
        initial: Duration::from_millis(50),
        max: Duration::from_secs(2),
        multiplier: 2,
        reset_after: Duration::from_secs(30),
    });
    let task =
        tokio::spawn(
            async move { idle::run(&source, timeout_rx, sink, backoff, &SystemClock).await },
        );
    let first = timeout(Duration::from_secs(20), rx.recv()).await.unwrap();
    assert_eq!(first, Some(ActivityEvent::InputIdle.into()));
    kwin.restart().await.unwrap();
    let again = timeout(Duration::from_secs(30), async {
        loop {
            match rx.recv().await {
                Some(event) if event == ActivityEvent::InputIdle.into() => return true,
                None => return false,
                Some(_) => {}
            }
        }
    })
    .await
    .unwrap();
    assert!(again, "idle source did not reconnect");
    assert!(!task.is_finished());
    task.abort();
}
