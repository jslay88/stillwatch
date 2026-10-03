//! Failures the GUI reports when the bus or a payload can't be used.

use stillwatch_ipc::error::IpcError;

/// Why a bus connection or a daemon call failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The session bus (or the address given on the command line) refused us.
    #[error("can't connect to the D-Bus session bus: {0}")]
    NoBus(#[source] zbus::Error),
    /// A method call, name request, or signal subscription failed.
    #[error("D-Bus call failed: {0}")]
    Bus(#[source] zbus::Error),
    /// The daemon's JSON wasn't a payload this build understands.
    #[error(transparent)]
    Payload(#[from] IpcError),
}

impl From<zbus::Error> for Error {
    fn from(err: zbus::Error) -> Self {
        Self::Bus(err)
    }
}
