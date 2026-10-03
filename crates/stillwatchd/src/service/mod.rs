//! The D-Bus control service: `io.github.jslay88.Stillwatch1` at
//! [`OBJECT_PATH`] under the well-known name [`BUS_NAME`].
//!
//! The service is a thin adapter. It checks arguments, turns calls into
//! [`DaemonHandle`] calls, and encodes the results with the `stillwatch-ipc`
//! wire types; the daemon implements the handle and emits signals through
//! [`ServiceSignals`].

#[cfg(any(test, feature = "fake"))]
pub mod fake;
mod handle;
mod interface;
mod probe;
mod signals;

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use stillwatch_ipc::error::IpcError;
use stillwatch_ipc::{BUS_NAME, OBJECT_PATH};
use zbus::Connection;
use zbus::fdo::RequestNameFlags;

pub use handle::{DaemonHandle, DaemonStatus, ReloadReport};
pub use signals::ServiceSignals;

use interface::Control;
use probe::ProbeHub;

/// Why the service couldn't start or emit a signal.
#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    /// Another process owns [`BUS_NAME`].
    #[error("another stillwatchd is already running ({BUS_NAME} is taken on the session bus)")]
    AlreadyRunning,
    /// Connecting, serving, or signalling failed.
    #[error("D-Bus error: {0}")]
    Bus(#[from] zbus::Error),
    /// A payload couldn't be encoded.
    #[error(transparent)]
    Payload(#[from] IpcError),
}

/// The running service. Calls are served on the connection until it's
/// dropped.
#[derive(Debug)]
pub struct Service {
    connection: Connection,
    signals: ServiceSignals,
}

impl Service {
    /// Connects to the session bus, serves the interface, and claims
    /// [`BUS_NAME`].
    ///
    /// # Errors
    ///
    /// Returns [`ServiceError::AlreadyRunning`] if another daemon owns the
    /// name, or [`ServiceError::Bus`] if the session bus is unreachable.
    pub async fn start(handle: Arc<dyn DaemonHandle>) -> Result<Self, ServiceError> {
        Self::claim(Connection::session().await?, handle).await
    }

    /// Serves the interface on a bus `connection` and claims [`BUS_NAME`]
    /// without queueing for it.
    ///
    /// # Errors
    ///
    /// Returns [`ServiceError::AlreadyRunning`] if another connection owns
    /// the name, or [`ServiceError::Bus`] if serving or the request fails.
    pub async fn claim(
        connection: Connection,
        handle: Arc<dyn DaemonHandle>,
    ) -> Result<Self, ServiceError> {
        let service = Self::serve(connection, handle).await?;
        match service
            .connection
            .request_name_with_flags(BUS_NAME, RequestNameFlags::DoNotQueue.into())
            .await
        {
            Ok(_) => Ok(service),
            Err(zbus::Error::NameTaken) => Err(ServiceError::AlreadyRunning),
            Err(err) => Err(err.into()),
        }
    }

    /// Serves the interface on `connection` without claiming a name, for
    /// peer-to-peer connections.
    ///
    /// # Errors
    ///
    /// Fails if the object path is already served on `connection`.
    pub async fn serve(
        connection: Connection,
        handle: Arc<dyn DaemonHandle>,
    ) -> Result<Self, ServiceError> {
        let signals = ServiceSignals::new(&connection)?;
        let control = Control::new(Arc::clone(&handle), ProbeHub::new(handle));
        if !connection.object_server().at(OBJECT_PATH, control).await? {
            return Err(zbus::Error::Failure(format!("{OBJECT_PATH} is already served")).into());
        }
        Ok(Self {
            connection,
            signals,
        })
    }

    /// The signal emitter for this service.
    #[must_use]
    pub const fn signals(&self) -> &ServiceSignals {
        &self.signals
    }

    /// The connection the service runs on.
    #[must_use]
    pub const fn connection(&self) -> &Connection {
        &self.connection
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
