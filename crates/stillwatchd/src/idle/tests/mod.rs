//! Drives the real Wayland client code against a fake compositor.

mod compositor;

use std::collections::VecDeque;
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use compositor::{FakeCompositor, IdleCompositor as _, Notification};
use stillwatch_core::backend::{BackendError, IdleSource};
use stillwatch_core::backoff::{Backoff, BackoffPolicy};
use stillwatch_core::event::{ActivityEvent, Event};
use stillwatch_core::mocks::RecordingSink;
use stillwatch_core::time::SystemClock;
use tokio::sync::watch;
use tokio::time::timeout;
use wayland_client::Connection;

use super::{V1_ONLY, WaylandIdleSource, run};

const TEN_MINUTES: Duration = Duration::from_secs(600);
const LIMIT: Duration = Duration::from_secs(5);

const KWIN: &[(u32, &str, u32)] = &[
    (1, "wl_compositor", 6),
    (11, "wl_seat", 10),
    (21, "ext_idle_notifier_v1", 2),
];

/// A source that connects to each socket in turn, then reports no compositor.
fn source(sockets: Vec<UnixStream>) -> WaylandIdleSource {
    let sockets = Mutex::new(VecDeque::from(sockets));
    WaylandIdleSource::with_connector(move || {
        let socket = sockets
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| BackendError::Disconnected("no compositor".into()))?;
        Ok(Connection::from_socket(socket).unwrap())
    })
}

/// Runs `script` as the compositor on its own thread.
fn serve<T: Send + 'static>(
    script: impl FnOnce(&mut FakeCompositor) -> std::io::Result<T> + Send + 'static,
) -> (JoinHandle<T>, UnixStream) {
    let (mut server, client) = FakeCompositor::pair();
    (thread::spawn(move || script(&mut server).unwrap()), client)
}

async fn watch_once(source: &WaylandIdleSource, sink: Arc<RecordingSink>) -> BackendError {
    timeout(LIMIT, source.watch(TEN_MINUTES, sink))
        .await
        .unwrap()
        .unwrap_err()
}

#[tokio::test]
async fn forwards_idle_and_resume_until_the_compositor_goes_away() {
    let (server, client) = serve(|compositor| {
        compositor.advertise(KWIN)?;
        let notification = compositor.accept_notification()?;
        compositor.idled(notification)?;
        compositor.resumed(notification)?;
        Ok(notification)
    });
    let sink = Arc::new(RecordingSink::new());
    let error = watch_once(&source(vec![client]), Arc::clone(&sink)).await;

    assert!(matches!(error, BackendError::Disconnected(_)), "{error}");
    assert_eq!(
        sink.events(),
        [
            ActivityEvent::InputIdle.into(),
            ActivityEvent::InputResumed.into()
        ]
    );
    let notification = server.join().unwrap();
    assert_eq!(notification.timeout_ms, 600_000);
    assert!(notification.seat_is_bound_seat);
}

#[tokio::test]
async fn v1_only_compositor_is_refused() {
    let (server, client) = serve(|compositor| {
        compositor.advertise(&[(11, "wl_seat", 10), (21, "ext_idle_notifier_v1", 1)])?;
        compositor.wait_for_close();
        Ok(())
    });
    let sink = Arc::new(RecordingSink::new());
    let error = watch_once(&source(vec![client]), Arc::clone(&sink)).await;

    assert_eq!(error, BackendError::Unsupported(V1_ONLY.into()));
    assert!(sink.is_empty());
    server.join().unwrap();
}

#[tokio::test]
async fn missing_notifier_is_unavailable() {
    let (server, client) = serve(|compositor| {
        compositor.advertise(&[(11, "wl_seat", 10)])?;
        compositor.wait_for_close();
        Ok(())
    });
    let error = watch_once(&source(vec![client]), Arc::new(RecordingSink::new())).await;
    assert!(matches!(error, BackendError::Unavailable(_)), "{error}");
    server.join().unwrap();
}

#[tokio::test]
async fn a_notifier_withdrawn_before_binding_is_unavailable() {
    let (server, client) = serve(|compositor| {
        compositor.advertise_then_remove(KWIN, &[21])?;
        compositor.wait_for_close();
        Ok(())
    });
    let error = watch_once(&source(vec![client]), Arc::new(RecordingSink::new())).await;
    assert!(
        matches!(&error, BackendError::Unavailable(m) if m.contains("ext_idle_notifier_v1")),
        "{error}"
    );
    server.join().unwrap();
}

#[tokio::test]
async fn protocol_errors_are_reported_as_such() {
    let (server, client) = serve(|compositor| {
        compositor.advertise(KWIN)?;
        let notification = compositor.accept_notification()?;
        compositor.protocol_error(notification.id, "idled twice")?;
        compositor.wait_for_close();
        Ok(())
    });
    let error = watch_once(&source(vec![client]), Arc::new(RecordingSink::new())).await;
    assert!(
        matches!(&error, BackendError::Protocol(m) if m.contains("idled twice")),
        "{error}"
    );
    server.join().unwrap();
}

#[tokio::test]
async fn a_closed_socket_before_the_registry_is_a_disconnect() {
    let (server, client) = serve(|_| Ok(()));
    server.join().unwrap();
    let error = watch_once(&source(vec![client]), Arc::new(RecordingSink::new())).await;
    assert!(error.is_transient(), "{error}");
}

#[tokio::test]
async fn reconnects_and_recreates_the_notification_after_a_restart() {
    let (first, first_client) = serve(|compositor| -> std::io::Result<Notification> {
        compositor.advertise(KWIN)?;
        let notification = compositor.accept_notification()?;
        compositor.idled(notification)?;
        Ok(notification)
    });
    let (second, second_client) = serve(|compositor| -> std::io::Result<Notification> {
        compositor.advertise(KWIN)?;
        let notification = compositor.accept_notification()?;
        compositor.idled(notification)?;
        compositor.wait_for_close();
        Ok(notification)
    });
    let source = Arc::new(source(vec![first_client, second_client]));
    let sink = Arc::new(RecordingSink::new());
    let (_tx, rx) = watch::channel(Duration::from_secs(90));
    let backoff = Backoff::new(BackoffPolicy {
        initial: Duration::from_millis(1),
        ..BackoffPolicy::default()
    });

    let task = {
        let (source, sink) = (Arc::clone(&source), Arc::clone(&sink));
        tokio::spawn(async move { run(&*source, rx, sink, backoff, &SystemClock).await })
    };
    let idle: Event = ActivityEvent::InputIdle.into();
    timeout(LIMIT, async {
        while sink.events() != [idle.clone(), idle.clone()] {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert!(!task.is_finished());
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());

    for server in [first, second] {
        assert_eq!(server.join().unwrap().timeout_ms, 90_000);
    }
}
