/// Why a backend operation failed.
///
/// Every variant carries a human-readable detail string, which keeps the type
/// `Clone + Eq` so it can travel inside events.
#[derive(Debug, Clone, PartialEq, Eq, Hash, thiserror::Error)]
pub enum BackendError {
    /// The service, protocol, or device isn't present.
    #[error("unavailable: {0}")]
    Unavailable(String),
    /// The backend exists but refused us.
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    /// The connection dropped. Retrying after a back-off may succeed.
    #[error("disconnected: {0}")]
    Disconnected(String),
    /// The other side sent something unexpected.
    #[error("protocol error: {0}")]
    Protocol(String),
    /// A named output, device, or player doesn't exist.
    #[error("not found: {0}")]
    NotFound(String),
    /// The backend can't do this, for example a pixel format it can't decode.
    #[error("unsupported: {0}")]
    Unsupported(String),
    /// An operating system I/O error.
    #[error("I/O error: {0}")]
    Io(String),
}

impl BackendError {
    /// Whether retrying (after a back-off) can reasonably succeed.
    #[must_use]
    pub const fn is_transient(&self) -> bool {
        matches!(self, Self::Disconnected(_) | Self::Io(_))
    }
}

impl From<std::io::Error> for BackendError {
    fn from(err: std::io::Error) -> Self {
        match err.kind() {
            std::io::ErrorKind::PermissionDenied => Self::PermissionDenied(err.to_string()),
            std::io::ErrorKind::NotFound => Self::NotFound(err.to_string()),
            _ => Self::Io(err.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::*;

    #[test]
    fn messages_include_the_detail() {
        let cases = [
            (
                BackendError::Unavailable("kwin".into()),
                "unavailable: kwin",
            ),
            (
                BackendError::PermissionDenied("x".into()),
                "permission denied: x",
            ),
            (BackendError::Disconnected("x".into()), "disconnected: x"),
            (BackendError::Protocol("x".into()), "protocol error: x"),
            (BackendError::NotFound("DP-9".into()), "not found: DP-9"),
            (
                BackendError::Unsupported("RGB16".into()),
                "unsupported: RGB16",
            ),
            (BackendError::Io("x".into()), "I/O error: x"),
        ];
        for (err, message) in cases {
            assert_eq!(err.to_string(), message);
        }
    }

    #[test]
    fn only_disconnects_and_io_are_transient() {
        assert!(BackendError::Disconnected(String::new()).is_transient());
        assert!(BackendError::Io(String::new()).is_transient());
        assert!(!BackendError::Protocol(String::new()).is_transient());
    }

    #[test]
    fn io_errors_map_by_kind() {
        let denied = io::Error::new(io::ErrorKind::PermissionDenied, "nope");
        assert_eq!(
            BackendError::from(denied),
            BackendError::PermissionDenied("nope".into())
        );
        let missing = io::Error::new(io::ErrorKind::NotFound, "gone");
        assert_eq!(
            BackendError::from(missing),
            BackendError::NotFound("gone".into())
        );
        let other = io::Error::other("boom");
        assert_eq!(BackendError::from(other), BackendError::Io("boom".into()));
    }
}
