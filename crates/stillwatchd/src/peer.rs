//! Waits for a well-known D-Bus name to change owner.
//!
//! Subscribe before reading the current owner, so a restart that lands in
//! between is still observed. This is not a retry loop: the caller decides
//! what a new owner means.

use futures_util::StreamExt as _;
use stillwatch_core::backend::BackendError;
use zbus::fdo::DBusProxy;
use zbus::names::BusName;
use zbus::{Connection, DBusError};

/// Resolves when `well_known` gains, loses, or replaces its owner.
///
/// `Ok` means the owner changed, including the name being released.
/// `Err` means the bus connection failed.
///
/// # Errors
///
/// [`BackendError::Disconnected`] when the bus connection fails, and
/// [`BackendError::Protocol`] when `well_known` is not a bus name.
pub(crate) async fn until_replaced(
    conn: &Connection,
    well_known: &str,
) -> Result<(), BackendError> {
    let dbus = DBusProxy::new(conn)
        .await
        .map_err(|err| BackendError::Disconnected(err.to_string()))?;
    let mut changes = dbus
        .receive_name_owner_changed_with_args(&[(0, well_known)])
        .await
        .map_err(|err| BackendError::Disconnected(err.to_string()))?;
    let name =
        BusName::try_from(well_known).map_err(|err| BackendError::Protocol(err.to_string()))?;
    let owner = match dbus.get_name_owner(name).await {
        Ok(owner) => Some(owner.to_string()),
        Err(err) if name_has_no_owner(&err) => None,
        Err(err) => return Err(BackendError::Disconnected(err.to_string())),
    };
    loop {
        let signal = changes.next().await.ok_or_else(|| {
            BackendError::Disconnected(format!("{well_known} owner stream ended"))
        })?;
        let args = signal
            .args()
            .map_err(|err| BackendError::Disconnected(err.to_string()))?;
        let next = args.new_owner().as_ref().map(ToString::to_string);
        if next != owner {
            return Ok(());
        }
    }
}

fn name_has_no_owner(err: &zbus::fdo::Error) -> bool {
    err.name() == "org.freedesktop.DBus.Error.NameHasNoOwner"
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[tokio::test]
    async fn resolves_when_the_owner_leaves() {
        let Ok(Some(bus)) = stillwatch_testkit::PrivateBus::start() else {
            return;
        };
        let owner = bus.connect().await.unwrap();
        owner
            .request_name("org.example.StillwatchPeer")
            .await
            .unwrap();
        let watcher = bus.connect().await.unwrap();
        let waiting =
            tokio::spawn(
                async move { until_replaced(&watcher, "org.example.StillwatchPeer").await },
            );
        tokio::time::sleep(Duration::from_millis(50)).await;
        drop(owner);
        let result = tokio::time::timeout(Duration::from_secs(2), waiting)
            .await
            .unwrap()
            .unwrap();
        assert!(result.is_ok(), "{result:?}");
    }
}
