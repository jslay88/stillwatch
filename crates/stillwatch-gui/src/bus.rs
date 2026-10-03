//! Opening the bus the GUI shares with the daemon.

use zbus::Connection;

use crate::error::Error;

/// Connects to `address`, or the session bus when `address` is `None`.
///
/// # Errors
///
/// Returns [`Error::NoBus`] when the bus can't be reached.
pub async fn connection(address: Option<&str>) -> Result<Connection, Error> {
    match address {
        Some(address) => zbus::connection::Builder::address(address)?.build().await,
        None => Connection::session().await,
    }
    .map_err(Error::NoBus)
}
