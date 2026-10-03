//! Blanking and dimming with a black layer-shell overlay.
//!
//! [`OverlayBlanker`] puts one `zwlr_layer_surface_v1` on each target output,
//! on the overlay layer, anchored to every edge with exclusive zone -1, no
//! keyboard interactivity, and an empty input region, so input still reaches
//! the compositor and whatever is underneath. The buffer is one `wl_shm`
//! pixel scaled up with `wp_viewporter`, or a full-size buffer without it.
//! A blank is opaque black; a dim is black at [`dim_alpha`].
//!
//! The overlay keeps the display's signal alive (so link-sensitive displays
//! stay dark), but the panel stays on, which blocks OLED panel compensation.
//!
//! [`watch`](Blanker::watch) owns the Wayland connection and must be running
//! (under [`supervise`](crate::supervise::supervise)) for `blank` and `dim`
//! to work. It reports `Event::DisplayPower { output, on: false }` when an
//! overlay starts showing and `on: true` when one goes away without an
//! unblank: closed by the compositor, its output removed, or the connection
//! lost (after which `watch` returns [`BackendError::Disconnected`]). What
//! should be covered is remembered across hotplug and reconnects, so an
//! output that comes back while blanked is covered again without a request.

mod handlers;
mod session;
mod shade;
mod state;
mod surface;
mod targets;

use std::sync::{Arc, Mutex};

use stillwatch_core::backend::{BackendError, BackendFuture, Blanker, Dimmer, EventSink};
use tokio::sync::{oneshot, watch};
use wayland_client::Connection;

pub use session::SHOW_TIMEOUT;
pub use shade::dim_alpha;
pub use surface::NAMESPACE;

use crate::wayland::{Connector, connect_to_env};
use session::{Link, Reply, Request};
use shade::Shade;
use state::lock;
use targets::{Desired, Lift};

/// How long a request waits for [`watch`](Blanker::watch) to connect.
pub const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// A [`Blanker`] and [`Dimmer`] backed by layer-shell overlays.
pub struct OverlayBlanker {
    connect: Box<Connector>,
    desired: Arc<Mutex<Desired>>,
    link: watch::Sender<Link>,
}

impl OverlayBlanker {
    /// Connects using `WAYLAND_DISPLAY` / `WAYLAND_SOCKET`, like any client.
    #[must_use]
    pub fn new() -> Self {
        Self::with_connector(connect_to_env)
    }

    /// Connects with `connect` instead of the environment.
    #[must_use]
    pub fn with_connector(
        connect: impl Fn() -> Result<Connection, BackendError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            connect: Box::new(connect),
            desired: Arc::new(Mutex::new(Desired::new())),
            link: watch::Sender::new(Link::Connecting),
        }
    }

    /// Sends `request` to the running session, waiting up to
    /// [`CONNECT_TIMEOUT`] for one, and returns its answer.
    async fn request(&self, request: impl FnOnce(Reply) -> Request) -> Result<(), BackendError> {
        let mut link = self.link.subscribe();
        let wait = async {
            loop {
                match &*link.borrow_and_update() {
                    Link::Up(session) => return Ok(session.clone()),
                    Link::Failed(error) => return Err(error.clone()),
                    Link::Connecting => {}
                }
                if link.changed().await.is_err() {
                    return Err(lost());
                }
            }
        };
        let session = tokio::time::timeout(CONNECT_TIMEOUT, wait)
            .await
            .map_err(|_| {
                BackendError::Disconnected("the overlay isn't connected to the compositor".into())
            })??;
        let (reply, answer) = oneshot::channel();
        session.send(request(reply)).map_err(|_| lost())?;
        answer.await.map_err(|_| lost())?
    }

    /// Forgets `outputs` per `lift` and takes their overlays down. Without a
    /// session there are no surfaces, so there's nothing else to do.
    async fn lift(&self, outputs: &[String], lift: Lift) -> Result<(), BackendError> {
        lock(&self.desired).uncover(outputs, lift);
        let Link::Up(session) = self.link.borrow().clone() else {
            return Ok(());
        };
        let (reply, answer) = oneshot::channel();
        if session.send(Request::Prune { reply }).is_err() {
            return Ok(());
        }
        answer.await.unwrap_or(Ok(()))
    }

    fn cover<'a>(&'a self, outputs: &'a [String], shade: Shade) -> BackendFuture<'a, ()> {
        Box::pin(self.request(move |reply| Request::Cover {
            outputs: outputs.to_vec(),
            shade,
            reply,
        }))
    }
}

impl Default for OverlayBlanker {
    fn default() -> Self {
        Self::new()
    }
}

fn lost() -> BackendError {
    BackendError::Disconnected("the overlay lost its compositor connection".into())
}

/// Resets a live link when its session ends or is cancelled.
struct LinkGuard<'a>(&'a watch::Sender<Link>);

impl Drop for LinkGuard<'_> {
    fn drop(&mut self) {
        self.0.send_if_modified(|link| {
            let up = matches!(link, Link::Up(_));
            if up {
                *link = Link::Connecting;
            }
            up
        });
    }
}

impl Blanker for OverlayBlanker {
    /// Covers `outputs` with opaque black. Returns once every connected
    /// target shows it. Named outputs that aren't connected are covered when
    /// they appear; if none of them are, it fails with
    /// [`BackendError::NotFound`].
    fn blank<'a>(&'a self, outputs: &'a [String]) -> BackendFuture<'a, ()> {
        self.cover(outputs, Shade::Black)
    }

    /// Takes every overlay (blank or dim) off `outputs`.
    fn unblank<'a>(&'a self, outputs: &'a [String]) -> BackendFuture<'a, ()> {
        Box::pin(self.lift(outputs, Lift::Any))
    }

    fn watch(&self, sink: Arc<dyn EventSink>) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            self.link.send_if_modified(|link| {
                let failed = matches!(link, Link::Failed(_));
                if failed {
                    *link = Link::Connecting;
                }
                failed
            });
            let _guard = LinkGuard(&self.link);
            let conn = (self.connect)();
            let result = match conn {
                Ok(conn) => session::run(&conn, sink, Arc::clone(&self.desired), &self.link).await,
                Err(error) => Err(error),
            };
            if let Err(error) = &result
                && !error.is_transient()
            {
                self.link.send_replace(Link::Failed(error.clone()));
            }
            result
        })
    }
}

impl Dimmer for OverlayBlanker {
    /// Covers `outputs` with black at [`dim_alpha`]`(percent)`, or replaces
    /// a blank with it. Otherwise as [`blank`](Blanker::blank).
    fn dim<'a>(&'a self, outputs: &'a [String], percent: u32) -> BackendFuture<'a, ()> {
        self.cover(outputs, Shade::dim(percent))
    }

    /// Takes dims off `outputs`; blanks stay.
    fn undim<'a>(&'a self, outputs: &'a [String]) -> BackendFuture<'a, ()> {
        Box::pin(self.lift(outputs, Lift::DimOnly))
    }
}

#[cfg(test)]
mod tests;
