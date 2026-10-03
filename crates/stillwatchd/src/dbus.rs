//! Connecting to D-Bus and turning bus errors into [`BackendError`]s.

use std::time::Duration;

use stillwatch_core::backend::BackendError;
use zbus::connection::Builder;
use zbus::{Connection, DBusError as _};

/// Which bus to connect to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Bus {
    /// The system bus (logind).
    System,
    /// The user's session bus.
    Session,
    /// A bus at an explicit address (tests use a private bus).
    Address(String),
}

impl Bus {
    /// How the bus is named in errors and logs.
    pub(crate) fn label(&self) -> &str {
        match self {
            Self::System => "system bus",
            Self::Session => "session bus",
            Self::Address(address) => address,
        }
    }
}

/// Opens a new connection to `bus`. Method calls on it give up after
/// `timeout`.
pub(crate) async fn connect(bus: &Bus, timeout: Duration) -> Result<Connection, BackendError> {
    let builder = match bus {
        Bus::System => Builder::system(),
        Bus::Session => Builder::session(),
        Bus::Address(address) => Builder::address(address.as_str()),
    }
    .map_err(|err| BackendError::Unavailable(format!("{}: {err}", bus.label())))?;
    builder
        .method_timeout(timeout)
        .build()
        .await
        .map_err(|err| BackendError::Disconnected(format!("{}: {err}", bus.label())))
}

/// Maps a failed call to `service` by the D-Bus error name it came back
/// with. Anything that isn't a reply from the bus or the service (a dropped
/// socket, a timeout) is transient.
pub(crate) fn call_error(service: &str, err: zbus::Error) -> BackendError {
    let detail = format!("{service}: {err}");
    let name = match err {
        zbus::Error::MethodError(name, _, _) => name.to_string(),
        zbus::Error::FDO(fdo) => fdo.name().to_string(),
        _ => return BackendError::Disconnected(detail),
    };
    match name.as_str() {
        "org.freedesktop.DBus.Error.ServiceUnknown"
        | "org.freedesktop.DBus.Error.NameHasNoOwner" => BackendError::Unavailable(detail),
        "org.freedesktop.DBus.Error.AccessDenied"
        | "org.freedesktop.DBus.Error.InteractiveAuthorizationRequired" => {
            BackendError::PermissionDenied(detail)
        }
        "org.freedesktop.DBus.Error.UnknownMethod"
        | "org.freedesktop.DBus.Error.UnknownInterface"
        | "org.freedesktop.DBus.Error.UnknownProperty" => BackendError::Unsupported(detail),
        "org.freedesktop.DBus.Error.UnknownObject"
        | "org.freedesktop.login1.NoSuchSession"
        | "org.freedesktop.login1.NoSessionForPID" => BackendError::NotFound(detail),
        "org.freedesktop.DBus.Error.NoReply"
        | "org.freedesktop.DBus.Error.Timeout"
        | "org.freedesktop.DBus.Error.Disconnected" => BackendError::Disconnected(detail),
        _ => BackendError::Protocol(detail),
    }
}

#[cfg(test)]
mod tests {
    use zbus::fdo;

    use super::*;

    #[test]
    fn labels_name_the_bus() {
        assert_eq!(Bus::System.label(), "system bus");
        assert_eq!(Bus::Session.label(), "session bus");
        assert_eq!(Bus::Address("unix:path=/x".into()).label(), "unix:path=/x");
    }

    #[test]
    fn fdo_errors_map_by_name() {
        let cases = [
            (
                fdo::Error::ServiceUnknown("x".into()),
                BackendError::Unavailable(String::new()),
            ),
            (
                fdo::Error::NameHasNoOwner("x".into()),
                BackendError::Unavailable(String::new()),
            ),
            (
                fdo::Error::AccessDenied("x".into()),
                BackendError::PermissionDenied(String::new()),
            ),
            (
                fdo::Error::InteractiveAuthorizationRequired("x".into()),
                BackendError::PermissionDenied(String::new()),
            ),
            (
                fdo::Error::UnknownMethod("x".into()),
                BackendError::Unsupported(String::new()),
            ),
            (
                fdo::Error::UnknownObject("x".into()),
                BackendError::NotFound(String::new()),
            ),
            (
                fdo::Error::NoReply("x".into()),
                BackendError::Disconnected(String::new()),
            ),
            (
                fdo::Error::Failed("x".into()),
                BackendError::Protocol(String::new()),
            ),
        ];
        for (fdo, expected) in cases {
            let mapped = call_error("svc", zbus::Error::FDO(Box::new(fdo)));
            assert_eq!(
                std::mem::discriminant(&mapped),
                std::mem::discriminant(&expected),
                "{mapped}"
            );
            assert!(mapped.to_string().contains("svc: "), "{mapped}");
        }
    }

    #[test]
    fn non_reply_errors_are_transient() {
        let err = call_error("svc", zbus::Error::Failure("socket closed".into()));
        assert!(err.is_transient(), "{err}");
    }
}
