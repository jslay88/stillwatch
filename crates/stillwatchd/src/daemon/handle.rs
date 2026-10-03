//! [`DaemonHandle`](crate::service::DaemonHandle) in front of the event loop.

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use futures_util::Stream;
use stillwatch_core::backend::{BackendError, BackendFuture, GamepadDevice, MediaPlayer};
use stillwatch_core::event::ControlCommand;
use stillwatch_core::history::HistoryEntry;
use stillwatch_core::prompt::PromptOutcome;
use stillwatch_core::state::{SnoozeError, validate_snooze};
use stillwatch_ipc::probe::ProbeSample;
use tokio::sync::{mpsc, oneshot};

use super::inbox::Incoming;
use super::shared::{Shared, lock};
use crate::config_watch::ReloadTrigger;
use crate::service::{DaemonHandle, DaemonStatus, ReloadReport};

/// The daemon as the D-Bus service sees it.
pub(super) struct Handle {
    shared: Arc<Shared>,
}

impl Handle {
    pub(super) const fn new(shared: Arc<Shared>) -> Self {
        Self { shared }
    }

    fn send(&self, message: Incoming) -> Result<(), BackendError> {
        self.shared.out.send(message).map_err(|_| stopped())
    }
}

impl DaemonHandle for Handle {
    fn status(&self) -> BackendFuture<'_, DaemonStatus> {
        let (reply, rx) = oneshot::channel();
        let sent = self.send(Incoming::Status(reply));
        Box::pin(async move {
            sent?;
            rx.await.map_err(|_| stopped())
        })
    }

    fn validate_snooze(&self, duration: Duration) -> Result<Duration, SnoozeError> {
        validate_snooze(&lock(&self.shared.prompt), duration)
    }

    fn control(&self, command: ControlCommand) -> BackendFuture<'_, ()> {
        let (reply, rx) = oneshot::channel();
        let sent = self.send(Incoming::Control(command, reply));
        Box::pin(async move {
            sent?;
            rx.await.map_err(|_| stopped())
        })
    }

    fn answer_prompt(&self, outcome: PromptOutcome) -> BackendFuture<'_, ()> {
        let (reply, rx) = oneshot::channel();
        let sent = self.send(Incoming::Answer(outcome, reply));
        Box::pin(async move {
            sent?;
            rx.await.map_err(|_| stopped())
        })
    }

    fn reload(&self) -> BackendFuture<'_, ReloadReport> {
        let (reply, rx) = oneshot::channel();
        let sent = self.send(Incoming::Reload(ReloadTrigger::Requested, Some(reply)));
        Box::pin(async move {
            sent?;
            rx.await.map_err(|_| stopped())
        })
    }

    fn history(&self, since: jiff::Timestamp) -> BackendFuture<'_, Vec<HistoryEntry>> {
        let history = Arc::clone(&self.shared.history);
        Box::pin(async move { history.read(since).await })
    }

    fn probe(&self, interval: Duration) -> futures_util::stream::BoxStream<'static, ProbeSample> {
        let (tx, rx) = mpsc::channel(4);
        if self.send(Incoming::Probe(interval, tx)).is_err() {
            return Box::pin(futures_util::stream::empty());
        }
        Box::pin(ProbeStream(rx))
    }

    fn outputs(&self) -> BackendFuture<'_, Vec<String>> {
        let capture = lock(&self.shared.capture).clone();
        Box::pin(async move {
            let Some(capture) = capture else {
                return Ok(Vec::new());
            };
            let outputs = capture.outputs().await?;
            Ok(outputs.into_iter().map(|output| output.name).collect())
        })
    }

    fn gamepads(&self) -> Vec<GamepadDevice> {
        lock(&self.shared.gamepad)
            .as_ref()
            .map(|source| source.devices())
            .unwrap_or_default()
    }

    fn players(&self) -> BackendFuture<'_, Vec<MediaPlayer>> {
        let media = Arc::clone(&self.shared.media);
        Box::pin(async move { media.players().await })
    }
}

fn stopped() -> BackendError {
    BackendError::Disconnected("stillwatchd is stopping".into())
}

/// The probe sample stream. Ends when the loop drops the sender.
struct ProbeStream(mpsc::Receiver<ProbeSample>);

impl Stream for ProbeStream {
    type Item = ProbeSample;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.get_mut().0.poll_recv(cx)
    }
}
