use std::sync::Arc;
use std::time::Duration;

use stillwatch_core::backend::{BackendError, EventSink};
use stillwatch_core::event::Event;
use stillwatch_testkit::kwin::{Kwin, KwinOptions};
use tokio::sync::mpsc;
use tokio::time::timeout;

use super::watch_on;

fn sink() -> (Arc<dyn EventSink>, mpsc::UnboundedReceiver<Event>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let sink = move |event| {
        let _ = tx.send(event);
    };
    (Arc::new(sink), rx)
}

async fn first_outputs(
    kwin: &Kwin,
    epoch: u64,
) -> Result<Vec<stillwatch_core::luma::OutputInfo>, BackendError> {
    let conn = kwin
        .connect()
        .map_err(|err| BackendError::Disconnected(err.to_string()))?;
    let (events, mut rx) = sink();
    let task = tokio::spawn(async move { watch_on(&conn, epoch, events).await });
    let event = timeout(Duration::from_secs(10), rx.recv())
        .await
        .map_err(|_| BackendError::Disconnected("no outputs".into()))?
        .ok_or_else(|| BackendError::Disconnected("watch ended".into()))?;
    task.abort();
    match event {
        Event::OutputsChanged(outputs) => Ok(outputs),
        other => Err(BackendError::Protocol(format!("unexpected {other:?}"))),
    }
}

#[tokio::test]
async fn a_restarted_compositor_gets_a_new_generation() {
    let Some(mut kwin) = Kwin::start(KwinOptions::default()).await.unwrap() else {
        return;
    };
    let first = first_outputs(&kwin, 0).await.unwrap();
    let virtual0 = first
        .iter()
        .find(|output| output.name == "Virtual-0")
        .unwrap();
    assert_eq!(virtual0.width, 1920);
    assert_eq!(virtual0.height, 1080);
    assert_eq!(virtual0.generation, 0);

    kwin.restart().await.unwrap();
    let again = first_outputs(&kwin, 1).await.unwrap();
    let virtual0 = again
        .iter()
        .find(|output| output.name == "Virtual-0")
        .unwrap();
    assert_eq!(virtual0.generation, 1_000_000);
}
