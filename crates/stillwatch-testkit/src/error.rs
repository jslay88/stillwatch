use crate::bus::REQUIRE_ENV;

/// Why a test helper couldn't start or drive its bus.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The bus daemon isn't installed and the environment requires it.
    #[error("{0} isn't installed, and {REQUIRE_ENV}=1 requires it")]
    Missing(String),
    /// The bus daemon exited or closed stdout before printing its address.
    #[error("dbus-daemon didn't print a bus address")]
    NoAddress,
    /// Spawning or talking to the bus daemon process failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// A D-Bus call or connection failed.
    #[error(transparent)]
    Bus(#[from] zbus::Error),
}
