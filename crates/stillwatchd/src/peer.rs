//! Waits for a well-known D-Bus name to change owner.
//!
//! Subscribe before reading the current owner, so a restart that lands in
//! between is still observed. This is not a retry loop: the caller decides
//! what a new owner means.

use futures_util::StreamExt as _;
use stillwatch_core::backend::BackendError;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use zbus::fdo::DBusProxy;
use zbus::names::BusName;
use zbus::{Connection, DBusError};

/// A subscription to owner changes of one well-known name.
///
/// [`NameWatch::arm`] records the current owner before returning, and keeps
/// reading signals from that moment. A release that lands before
/// [`until_changed`](NameWatch::until_changed) is polled is still delivered.
pub(crate) struct NameWatch {
    owner: Option<String>,
    changed: mpsc::Receiver<Result<(), BackendError>>,
    task: JoinHandle<()>,
}

impl NameWatch {
    /// Subscribes to `well_known` and snapshots its owner.
    ///
    /// # Errors
    ///
    /// [`BackendError::Disconnected`] when the bus connection fails, and
    /// [`BackendError::Protocol`] when `well_known` is not a bus name.
    pub(crate) async fn arm(conn: &Connection, well_known: &str) -> Result<Self, BackendError> {
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
        let (tx, rx) = mpsc::channel(1);
        let snapshot = owner.clone();
        let watched = well_known.to_owned();
        let task = tokio::spawn(async move {
            let result = async {
                loop {
                    let signal = changes.next().await.ok_or_else(|| {
                        BackendError::Disconnected(format!("{watched} owner stream ended"))
                    })?;
                    let args = signal
                        .args()
                        .map_err(|err| BackendError::Disconnected(err.to_string()))?;
                    let next = args.new_owner().as_ref().map(ToString::to_string);
                    if next != snapshot {
                        return Ok(());
                    }
                }
            }
            .await;
            let _ = tx.send(result).await;
            drop(dbus);
        });
        Ok(Self {
            owner,
            changed: rx,
            task,
        })
    }

    /// Owner at [`arm`](Self::arm) time. `None` means the name was free.
    #[must_use]
    pub(crate) fn owner(&self) -> Option<&str> {
        self.owner.as_deref()
    }

    /// Resolves when the owner differs from [`owner`](Self::owner).
    ///
    /// # Errors
    ///
    /// [`BackendError::Disconnected`] when the bus connection fails or the
    /// watch task ends first.
    pub(crate) async fn until_changed(&mut self) -> Result<(), BackendError> {
        self.changed.recv().await.ok_or_else(|| {
            BackendError::Disconnected("owner watch ended before the name changed".into())
        })?
    }
}

impl Drop for NameWatch {
    fn drop(&mut self) {
        self.task.abort();
    }
}

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
    let mut watch = NameWatch::arm(conn, well_known).await?;
    watch.until_changed().await
}

fn name_has_no_owner(err: &zbus::fdo::Error) -> bool {
    err.name() == "org.freedesktop.DBus.Error.NameHasNoOwner"
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    const NAME: &str = "org.example.StillwatchPeer";

    async fn held_name() -> Option<(stillwatch_testkit::PrivateBus, Connection, Connection)> {
        let Ok(Some(bus)) = stillwatch_testkit::PrivateBus::start() else {
            return None;
        };
        let owner = bus.connect().await.unwrap();
        owner.request_name(NAME).await.unwrap();
        let watcher = bus.connect().await.unwrap();
        Some((bus, owner, watcher))
    }

    #[tokio::test]
    async fn resolves_when_the_owner_leaves() {
        let Some((_bus, owner, watcher)) = held_name().await else {
            return;
        };
        let waiting = tokio::spawn(async move { until_replaced(&watcher, NAME).await });
        tokio::time::sleep(Duration::from_millis(50)).await;
        drop(owner);
        let result = tokio::time::timeout(Duration::from_secs(2), waiting)
            .await
            .unwrap()
            .unwrap();
        assert!(result.is_ok(), "{result:?}");
    }

    #[tokio::test]
    async fn a_release_before_the_wait_is_polled_is_still_observed() {
        let Some((_bus, owner, watcher)) = held_name().await else {
            return;
        };
        let mut watch = NameWatch::arm(&watcher, NAME).await.unwrap();
        assert!(watch.owner().is_some());
        drop(owner);
        tokio::time::sleep(Duration::from_millis(50)).await;
        let result = tokio::time::timeout(Duration::from_secs(2), watch.until_changed())
            .await
            .unwrap();
        assert!(result.is_ok(), "{result:?}");
    }
}
