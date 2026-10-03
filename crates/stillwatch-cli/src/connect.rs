//! Reaching the daemon, and turning its D-Bus errors into messages for the
//! user.

use stillwatch_ipc::error::IpcError;
use stillwatch_ipc::proxy::StillwatchProxy;
use zbus::proxy::CacheProperties;
use zbus::{Connection, connection, fdo};

/// What the user sees when nothing owns the daemon's bus name.
pub const NOT_RUNNING: &str =
    "stillwatchd is not running; start it with systemctl --user start stillwatch";

/// Why a daemon command couldn't get an answer.
#[derive(Debug, thiserror::Error)]
pub enum DaemonError {
    /// Nothing owns the daemon's bus name.
    #[error("{NOT_RUNNING}")]
    NotRunning,
    /// The daemon left the bus while the command was waiting on it.
    #[error("stillwatchd stopped; start it again with systemctl --user start stillwatch")]
    Stopped,
    /// The daemon rejected the request (`InvalidArgs` or `Failed`). The
    /// message is written for the user.
    #[error("{0}")]
    Refused(String),
    /// The bus itself couldn't be reached.
    #[error("can't connect to the D-Bus session bus: {0}")]
    NoBus(#[source] zbus::Error),
    /// Any other D-Bus failure.
    #[error("D-Bus call failed: {0}")]
    Bus(#[source] zbus::Error),
    /// The daemon answered with a payload this CLI can't read.
    #[error("stillwatchd sent a reply this stillwatch can't read: {0}")]
    Reply(#[from] IpcError),
}

impl DaemonError {
    /// Whether the error means the daemon isn't there.
    #[must_use]
    pub const fn is_not_running(&self) -> bool {
        matches!(self, Self::NotRunning | Self::Stopped)
    }
}

impl From<zbus::Error> for DaemonError {
    fn from(err: zbus::Error) -> Self {
        match fdo::Error::from(err) {
            fdo::Error::ServiceUnknown(_) | fdo::Error::NameHasNoOwner(_) => Self::NotRunning,
            fdo::Error::InvalidArgs(message) | fdo::Error::Failed(message) => {
                Self::Refused(message)
            }
            other => Self::Bus(other.into()),
        }
    }
}

/// Connects to the bus at `address` (the session bus when `None`) and
/// returns a proxy for the daemon.
///
/// Connecting doesn't check that the daemon is running; the first call does,
/// and fails with [`DaemonError::NotRunning`] if it isn't.
///
/// # Errors
///
/// Returns [`DaemonError::NoBus`] if the bus can't be reached.
pub async fn connect(address: Option<&str>) -> Result<StillwatchProxy<'static>, DaemonError> {
    let connection = match address {
        Some(address) => open(address).await,
        None => Connection::session().await,
    }
    .map_err(DaemonError::NoBus)?;
    Ok(StillwatchProxy::builder(&connection)
        .cache_properties(CacheProperties::No)
        .build()
        .await?)
}

async fn open(address: &str) -> zbus::Result<Connection> {
    connection::Builder::address(address)?.build().await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn method_error(name: &str, message: &str) -> zbus::Error {
        zbus::Error::from(fdo::Error::from(zbus::Error::MethodError(
            name.try_into().unwrap(),
            Some(message.to_owned()),
            zbus::message::Message::method_call("/", "Ping")
                .unwrap()
                .build(&())
                .unwrap(),
        )))
    }

    #[test]
    fn unowned_names_mean_not_running() {
        for err in [
            zbus::Error::from(fdo::Error::ServiceUnknown("no .service file".into())),
            zbus::Error::from(fdo::Error::NameHasNoOwner("gone".into())),
            method_error("org.freedesktop.DBus.Error.ServiceUnknown", "nope"),
        ] {
            let err = DaemonError::from(err);
            assert!(matches!(err, DaemonError::NotRunning), "{err:?}");
            assert!(err.is_not_running());
            assert_eq!(err.to_string(), NOT_RUNNING);
        }
        assert!(DaemonError::Stopped.is_not_running());
    }

    #[test]
    fn refusals_keep_the_daemon_message() {
        let err = DaemonError::from(zbus::Error::from(fdo::Error::InvalidArgs(
            "snooze must be at most 720 minutes".into(),
        )));
        assert_eq!(err.to_string(), "snooze must be at most 720 minutes");
        let err = DaemonError::from(method_error(
            "org.freedesktop.DBus.Error.Failed",
            "capture failed",
        ));
        assert_eq!(err.to_string(), "capture failed");
        assert!(!err.is_not_running());
    }

    #[test]
    fn other_errors_are_bus_failures() {
        let err = DaemonError::from(zbus::Error::from(fdo::Error::UnknownMethod(
            "no Frobnicate".into(),
        )));
        assert!(matches!(err, DaemonError::Bus(_)), "{err:?}");
        assert!(err.to_string().starts_with("D-Bus call failed: "), "{err}");
        assert!(err.to_string().contains("no Frobnicate"), "{err}");
    }

    #[tokio::test]
    async fn a_bad_address_is_no_bus() {
        let err = connect(Some("not an address")).await.unwrap_err();
        assert!(matches!(err, DaemonError::NoBus(_)), "{err:?}");
        let dir = tempfile::tempdir().unwrap();
        let address = format!("unix:path={}", dir.path().join("missing").display());
        let err = connect(Some(&address)).await.unwrap_err();
        assert!(
            err.to_string()
                .starts_with("can't connect to the D-Bus session bus: "),
            "{err}"
        );
    }
}
