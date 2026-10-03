//! Screen capture through the xdg-desktop-portal `ScreenCast` and `PipeWire`.
//!
//! The portal shows a screen-sharing indicator for the whole session, so the
//! stream runs only while [`Presence`] is [`Away`](Presence::Away): idle in
//! Monitoring, or idle in Snoozed or Paused while the ceiling is capturing.
//! Becoming active closes the session. The restore token in
//! `~/.local/state/stillwatch/portal-restore-token` lets later sessions skip
//! the monitor picker.
//!
//! [`PortalCapture::connect`] only checks that the portal is on the bus. It
//! does not call `Start`, so it does not open a permission dialog.

mod cache;
mod error;
mod frame;
mod handshake;
mod lifecycle;
mod map;
mod pipewire;
mod token;

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use futures_util::StreamExt as _;
use stillwatch_core::backend::{BackendError, BackendFuture, ScreenCapture};
use stillwatch_core::luma::{LumaGrid, OutputInfo};

use crate::clock::TokioClock;
use crate::dbus::{self, Bus};
use crate::outputs::PlacedOutput;

pub use handshake::open_session;
pub use lifecycle::{CaptureWant, CeilingCapture, Presence, presence_for};
pub use token::{FILE_NAME, TokenStore};

use cache::FrameCache;
use handshake::Opened;
use lifecycle::{Order, SessionMachine};
use map::assign;
use pipewire::{PipeThread, Target};

type ListOutputs = dyn Fn() -> BackendFuture<'static, Vec<PlacedOutput>> + Send + Sync;

/// How long [`ScreenCapture::capture_luma`] waits for a frame.
const CAPTURE_WAIT: Duration = Duration::from_secs(3);

/// How long a portal call may take, including the permission dialog.
const CALL_TIMEOUT: Duration = Duration::from_secs(120);

/// A [`ScreenCapture`] backed by the desktop portal.
#[derive(Clone)]
pub struct PortalCapture {
    shared: Arc<Shared>,
}

struct Shared {
    conn: zbus::Connection,
    tokens: TokenStore,
    outputs: Box<ListOutputs>,
    cache: Arc<FrameCache>,
    inner: Mutex<Inner>,
    clock: TokioClock,
}

struct Inner {
    machine: SessionMachine,
    generation: u64,
    live: Option<Live>,
}

struct Live {
    pipe: PipeThread,
    close: Option<tokio::sync::oneshot::Sender<()>>,
}

impl PortalCapture {
    /// Connects to the session bus and checks that `ScreenCast` is there.
    ///
    /// Does not start a stream.
    ///
    /// # Errors
    ///
    /// [`BackendError::Unavailable`] when the portal isn't on the session bus,
    /// and [`BackendError::Io`] when the restore-token path can't be resolved.
    pub async fn connect() -> Result<Self, BackendError> {
        let conn = dbus::connect(&Bus::Session, CALL_TIMEOUT).await?;
        let tokens = TokenStore::new(
            TokenStore::default_path().map_err(|err| BackendError::Io(err.to_string()))?,
        );
        Self::on(conn, tokens, || Box::pin(crate::outputs::list_placed())).await
    }

    /// Uses `conn` (a private bus in tests) and `tokens`.
    ///
    /// # Errors
    ///
    /// [`BackendError::Unavailable`] when `conn` has no `ScreenCast` portal.
    pub async fn on(
        conn: zbus::Connection,
        tokens: TokenStore,
        outputs: impl Fn() -> BackendFuture<'static, Vec<PlacedOutput>> + Send + Sync + 'static,
    ) -> Result<Self, BackendError> {
        // Building the proxy reads the version property. `Start` is separate,
        // so this does not open a dialog.
        let _proxy = ashpd::desktop::screencast::Screencast::with_connection(conn.clone())
            .await
            .map_err(error::map_ashpd)?;
        Ok(Self {
            shared: Arc::new(Shared {
                conn,
                tokens,
                outputs: Box::new(outputs),
                cache: Arc::new(FrameCache::new()),
                inner: Mutex::new(Inner {
                    machine: SessionMachine::new(),
                    generation: 0,
                    live: None,
                }),
                clock: TokioClock,
            }),
        })
    }

    /// Starts the stream when `presence` is away, and stops it when active.
    ///
    /// # Errors
    ///
    /// Whatever starting the session failed with. A denial or a missing portal
    /// disables further attempts until the daemon restarts.
    pub async fn set_presence(&self, presence: Presence) -> Result<(), BackendError> {
        let order = {
            let mut inner = lock(&self.shared.inner);
            inner.machine.set_presence(presence, &self.shared.clock)
        };
        self.apply(order).await
    }

    /// Whether a stream is up.
    #[must_use]
    pub fn streaming(&self) -> bool {
        lock(&self.shared.inner).machine.streaming()
    }

    async fn apply(&self, order: Order) -> Result<(), BackendError> {
        match order {
            Order::Start => self.start().await,
            Order::Stop => {
                self.stop_live().await;
                Ok(())
            }
            Order::None => Ok(()),
        }
    }

    async fn start(&self) -> Result<(), BackendError> {
        let generation = lock(&self.shared.inner).generation;
        let restore = self.shared.tokens.load()?;
        let opened = match open_session(&self.shared.conn, restore.as_deref()).await {
            Ok(opened) => opened,
            Err(error) => {
                self.fail(error.clone()).await;
                return Err(error);
            }
        };
        if let Err(error) = self.shared.tokens.store(opened.restore_token.as_deref()) {
            tracing::warn!(%error, "couldn't save the portal restore token");
        }
        if lock(&self.shared.inner).generation != generation {
            let _ = opened.close().await;
            return Ok(());
        }
        self.attach(opened, generation).await
    }

    async fn attach(&self, opened: Opened, generation: u64) -> Result<(), BackendError> {
        let outputs = (self.shared.outputs)().await?;
        let assigned = assign(&opened.streams, &outputs);
        if assigned.is_empty() {
            let error =
                BackendError::NotFound("no portal stream matched a connected output".into());
            let _ = opened.close().await;
            self.fail(error.clone()).await;
            return Err(error);
        }
        let targets = assigned
            .into_iter()
            .map(|assignment| Target {
                node_id: assignment.node_id,
                output: assignment.output,
            })
            .collect();
        self.shared.cache.clear();
        let remote = opened.remote;
        let session = opened.session;
        let pipe = match PipeThread::spawn(remote, targets, Arc::clone(&self.shared.cache)).await {
            Ok(pipe) => pipe,
            Err(error) => {
                drop(session);
                self.fail(error.clone()).await;
                return Err(error);
            }
        };
        if lock(&self.shared.inner).generation != generation {
            drop(pipe);
            drop(session);
            return Ok(());
        }
        let (close_tx, close_rx) = tokio::sync::oneshot::channel();
        self.watch(session, close_rx, generation);
        {
            let mut inner = lock(&self.shared.inner);
            inner.machine.started(&self.shared.clock);
            inner.live = Some(Live {
                pipe,
                close: Some(close_tx),
            });
        }
        Ok(())
    }

    fn watch(
        &self,
        session: ashpd::desktop::Session<ashpd::desktop::screencast::Screencast>,
        close_rx: tokio::sync::oneshot::Receiver<()>,
        generation: u64,
    ) {
        let this = self.clone();
        tokio::spawn(async move {
            let mut closed = match session.receive_closed().await {
                Ok(closed) => closed,
                Err(error) => {
                    this.fail(error::map_ashpd(error)).await;
                    return;
                }
            };
            let ended = tokio::select! {
                _ = close_rx => false,
                event = closed.next() => event.is_some(),
            };
            if ended && lock(&this.shared.inner).generation == generation {
                this.fail(BackendError::Disconnected(
                    "the compositor closed the screen cast session".into(),
                ))
                .await;
            }
            let _ = session.close().await;
        });
    }

    async fn fail(&self, error: BackendError) {
        let delay = {
            let mut inner = lock(&self.shared.inner);
            let _order = inner.machine.failed(&error, &self.shared.clock);
            if inner.machine.blocked() {
                tracing::info!(%error, "portal capture stays off until restart");
            }
            inner.machine.retry_in(&self.shared.clock)
        };
        self.stop_live().await;
        let Some(delay) = delay else {
            return;
        };
        schedule_retry(self.clone(), delay);
    }

    async fn stop_live(&self) {
        let live = {
            let mut inner = lock(&self.shared.inner);
            inner.generation = inner.generation.wrapping_add(1);
            inner.live.take()
        };
        if let Some(mut live) = live {
            if let Some(close) = live.close.take() {
                let _ = close.send(());
            }
            let pipe = live.pipe;
            let _ = tokio::task::spawn_blocking(move || {
                let mut pipe = pipe;
                pipe.stop();
            })
            .await;
        }
        self.shared.cache.clear();
    }
}

/// Retries outside [`PortalCapture::fail`] so the retry task and `fail` don't
/// form one recursive future.
fn schedule_retry(this: PortalCapture, delay: Duration) {
    tokio::spawn(async move {
        tokio::time::sleep(delay).await;
        let order = {
            let mut inner = lock(&this.shared.inner);
            inner.machine.poll(&this.shared.clock)
        };
        if let Err(error) = this.apply(order).await {
            tracing::info!(%error, "portal capture retry failed");
        }
    });
}

impl ScreenCapture for PortalCapture {
    fn outputs(&self) -> BackendFuture<'_, Vec<OutputInfo>> {
        let list = (self.shared.outputs)();
        Box::pin(async move {
            list.await
                .map(|outputs| outputs.into_iter().map(|output| output.info).collect())
        })
    }

    fn capture_luma<'a>(
        &'a self,
        output: &'a str,
        downscale_width: u32,
    ) -> BackendFuture<'a, LumaGrid> {
        Box::pin(async move {
            if !self.streaming() {
                return Err(BackendError::Unavailable(
                    "portal capture is stopped while you're active".into(),
                ));
            }
            self.shared.cache.set_width(downscale_width);
            let deadline = tokio::time::Instant::now() + CAPTURE_WAIT;
            loop {
                if let Some(error) = self.shared.cache.fault() {
                    return Err(error);
                }
                if let Some(grid) = self.shared.cache.get(output, downscale_width) {
                    return Ok(grid);
                }
                if !self.streaming() {
                    return Err(BackendError::Unavailable(
                        "portal capture stopped while waiting for a frame".into(),
                    ));
                }
                let notified = self.shared.cache.notify().notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if let Some(grid) = self.shared.cache.get(output, downscale_width) {
                    return Ok(grid);
                }
                tokio::select! {
                    () = notified => {}
                    () = tokio::time::sleep_until(deadline) => {
                        return Err(BackendError::Io(format!(
                            "no portal frame for {output} within {}s",
                            CAPTURE_WAIT.as_secs()
                        )));
                    }
                }
            }
        })
    }
}

fn lock(inner: &Mutex<Inner>) -> MutexGuard<'_, Inner> {
    inner.lock().unwrap_or_else(PoisonError::into_inner)
}
