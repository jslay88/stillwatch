//! Keyboard and mouse idle from the compositor via `ext-idle-notify-v1`.
//!
//! [`WaylandIdleSource`] binds `ext_idle_notifier_v1` at version 2 or later
//! and asks for an *input* idle notification, which ignores idle inhibitors
//! (a video call holding one doesn't keep the user "active"). A compositor
//! with only v1 is refused rather than silently falling back to the
//! inhibitor-respecting notification.
//!
//! [`run`] keeps it alive: it reconnects with back-off when the compositor
//! restarts and recreates the notification when the timeout changes.

mod protocol;
mod runner;
mod session;

use std::sync::Arc;
use std::time::Duration;

use stillwatch_core::backend::{BackendError, BackendFuture, EventSink, IdleSource};
use wayland_client::Connection;

pub use protocol::{MIN_NOTIFIER_VERSION, V1_ONLY};
pub use runner::run;

use crate::wayland::connect_error;

type Connector = dyn Fn() -> Result<Connection, BackendError> + Send + Sync;

/// An [`IdleSource`] backed by the Wayland compositor's
/// `ext_idle_notifier_v1` (v2+) `get_input_idle_notification`.
///
/// Each [`watch`](IdleSource::watch) opens its own connection, so dropping
/// the future closes it and the compositor forgets the notification.
pub struct WaylandIdleSource {
    connect: Box<Connector>,
}

impl WaylandIdleSource {
    /// Connects using `WAYLAND_DISPLAY` / `WAYLAND_SOCKET`, like any client.
    #[must_use]
    pub fn new() -> Self {
        Self::with_connector(|| Connection::connect_to_env().map_err(|e| connect_error(&e)))
    }

    /// Connects with `connect` instead of the environment.
    #[must_use]
    pub fn with_connector(
        connect: impl Fn() -> Result<Connection, BackendError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            connect: Box::new(connect),
        }
    }
}

impl Default for WaylandIdleSource {
    fn default() -> Self {
        Self::new()
    }
}

impl IdleSource for WaylandIdleSource {
    fn watch(&self, timeout: Duration, sink: Arc<dyn EventSink>) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            let conn = (self.connect)()?;
            session::watch_on(&conn, timeout, sink).await
        })
    }
}

#[cfg(test)]
mod tests;
