//! The blanker's request plumbing without a compositor. The overlays
//! themselves are tested against `KWin` in `tests/overlay_kwin.rs`.

use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use stillwatch_core::mocks::RecordingSink;

use super::*;

fn refused() -> OverlayBlanker {
    OverlayBlanker::with_connector(|| Err(BackendError::Unavailable("no compositor here".into())))
}

fn sink() -> Arc<dyn EventSink> {
    Arc::new(RecordingSink::new())
}

#[tokio::test(start_paused = true)]
async fn requests_without_a_session_time_out_as_disconnected() {
    let blanker = OverlayBlanker::with_connector(|| unreachable!("watch never runs"));
    let error = blanker.blank(&[]).await.unwrap_err();
    assert!(matches!(error, BackendError::Disconnected(_)), "{error}");
    let error = blanker.dim(&[], 20).await.unwrap_err();
    assert!(error.is_transient(), "{error}");
}

#[tokio::test(start_paused = true)]
async fn a_permanent_failure_is_reported_to_requests_right_away() {
    let blanker = refused();
    let ended = blanker.watch(sink()).await;
    assert_eq!(
        ended,
        Err(BackendError::Unavailable("no compositor here".into()))
    );

    let started = tokio::time::Instant::now();
    let error = blanker.blank(&[]).await.unwrap_err();
    assert_eq!(
        error,
        BackendError::Unavailable("no compositor here".into())
    );
    assert!(started.elapsed() < CONNECT_TIMEOUT);
}

#[tokio::test(start_paused = true)]
async fn a_transient_failure_leaves_requests_waiting_for_a_reconnect() {
    let blanker = OverlayBlanker::with_connector(|| {
        Err(BackendError::Disconnected("compositor restarting".into()))
    });
    assert!(blanker.watch(sink()).await.unwrap_err().is_transient());
    assert!(matches!(*blanker.link.borrow(), Link::Connecting));
    let error = blanker.blank(&[]).await.unwrap_err();
    assert!(matches!(error, BackendError::Disconnected(_)), "{error}");
}

#[tokio::test(start_paused = true)]
async fn a_new_watch_clears_an_old_failure() {
    let blanker = refused();
    let _ = blanker.watch(sink()).await;
    assert!(matches!(*blanker.link.borrow(), Link::Failed(_)));
    blanker.link.send_modify(|link| {
        if let Link::Failed(error) = link {
            *error = BackendError::Disconnected("stale".into());
        }
    });
    let _ = blanker.watch(sink()).await;
    assert!(matches!(
        &*blanker.link.borrow(),
        Link::Failed(BackendError::Unavailable(_))
    ));
}

#[tokio::test]
async fn unblank_and_undim_without_a_session_forget_the_targets() {
    let blanker = refused();
    lock(&blanker.desired).cover(&[], Shade::Black);
    blanker.undim(&[]).await.unwrap();
    assert_eq!(lock(&blanker.desired).shade_for("DP-1"), Some(Shade::Black));
    blanker.unblank(&[]).await.unwrap();
    assert_eq!(lock(&blanker.desired).shade_for("DP-1"), None);
}

#[tokio::test]
async fn unblank_succeeds_when_the_session_is_gone() {
    let blanker = refused();
    let (session, inbox) = tokio::sync::mpsc::unbounded_channel();
    drop(inbox);
    blanker.link.send_replace(Link::Up(session));
    lock(&blanker.desired).cover(&[], Shade::dim(20));
    blanker.unblank(&[]).await.unwrap();
    assert_eq!(lock(&blanker.desired).shade_for("DP-1"), None);
    let error = blanker.blank(&[]).await.unwrap_err();
    assert!(error.is_transient(), "{error}");
}

#[tokio::test]
async fn a_compositor_that_hangs_up_is_a_disconnect() {
    let (client, server) = UnixStream::pair().unwrap();
    drop(server);
    let socket = Mutex::new(Some(client));
    let blanker = OverlayBlanker::with_connector(move || {
        let stream = socket.lock().unwrap().take().unwrap();
        Connection::from_socket(stream)
            .map_err(|error| BackendError::Unavailable(error.to_string()))
    });
    let ended = tokio::time::timeout(Duration::from_secs(10), blanker.watch(sink()))
        .await
        .unwrap();
    assert!(
        matches!(ended, Err(BackendError::Disconnected(_))),
        "{ended:?}"
    );
    assert!(matches!(*blanker.link.borrow(), Link::Connecting));
}

#[test]
fn default_starts_unconnected() {
    let blanker = OverlayBlanker::default();
    assert!(matches!(*blanker.link.borrow(), Link::Connecting));
}
