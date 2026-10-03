//! Why a DDC/CI operation failed.

use std::time::Duration;

use stillwatch_core::backend::BackendError;

/// Why a DDC/CI operation failed. Every variant becomes a
/// [`BackendError`] the daemon logs and falls back from; none is fatal.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DdcError {
    /// i2c device nodes exist but the user can't open them.
    #[error(
        "no permission to open {}; add your user to the `i2c` group (or install a udev rule \
         that grants access to /dev/i2c-*), then log in again",
        .nodes.join(", ")
    )]
    NoAccess {
        /// The device nodes that refused us.
        nodes: Vec<String>,
    },
    /// No DDC/CI display could be matched to the output.
    #[error("no DDC/CI display for output {output}: {reason}")]
    DisplayNotFound {
        /// Connector name.
        output: String,
        /// What was missing.
        reason: String,
    },
    /// Setting a VCP feature failed after every retry.
    #[error("writing VCP {code:#04x} = {value:#04x} to {output} failed: {detail}")]
    WriteFailed {
        /// Connector name.
        output: String,
        /// VCP feature code.
        code: u8,
        /// The value being written.
        value: u16,
        /// The transport's error.
        detail: String,
    },
    /// Reading a VCP feature failed after every retry.
    #[error("reading VCP {code:#04x} from {output} failed: {detail}")]
    ReadFailed {
        /// Connector name.
        output: String,
        /// VCP feature code.
        code: u8,
        /// The transport's error.
        detail: String,
    },
    /// An operation took longer than allowed.
    #[error("DDC/CI {op} timed out after {after:?}")]
    Timeout {
        /// What was running.
        op: String,
        /// The limit it hit.
        after: Duration,
    },
    /// DDC/CI can't be used at all right now.
    #[error("DDC/CI unavailable: {0}")]
    Unavailable(String),
}

impl DdcError {
    /// Whether another attempt can reasonably succeed. Timeouts aren't
    /// retried: the stuck call still holds the bus.
    #[must_use]
    pub const fn is_retryable(&self) -> bool {
        matches!(self, Self::WriteFailed { .. } | Self::ReadFailed { .. })
    }
}

impl From<DdcError> for BackendError {
    fn from(error: DdcError) -> Self {
        let message = error.to_string();
        match error {
            DdcError::NoAccess { .. } => Self::PermissionDenied(message),
            DdcError::DisplayNotFound { .. } => Self::NotFound(message),
            DdcError::WriteFailed { .. }
            | DdcError::ReadFailed { .. }
            | DdcError::Timeout { .. } => Self::Io(message),
            DdcError::Unavailable(_) => Self::Unavailable(message),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_access_points_at_the_i2c_group() {
        let error = DdcError::NoAccess {
            nodes: vec!["/dev/i2c-3".into(), "/dev/i2c-4".into()],
        };
        let message = error.to_string();
        assert!(message.starts_with("no permission to open /dev/i2c-3, /dev/i2c-4;"));
        assert!(message.contains("`i2c` group"));
        assert!(message.contains("udev rule"));
        assert_eq!(
            BackendError::from(error),
            BackendError::PermissionDenied(message)
        );
    }

    #[test]
    fn maps_onto_backend_errors() {
        let not_found = DdcError::DisplayNotFound {
            output: "DP-1".into(),
            reason: "not connected".into(),
        };
        assert_eq!(
            BackendError::from(not_found),
            BackendError::NotFound("no DDC/CI display for output DP-1: not connected".into())
        );
        let write = DdcError::WriteFailed {
            output: "HDMI-A-1".into(),
            code: 0xD6,
            value: 4,
            detail: "NAK".into(),
        };
        assert_eq!(
            BackendError::from(write),
            BackendError::Io("writing VCP 0xd6 = 0x04 to HDMI-A-1 failed: NAK".into())
        );
        let read = DdcError::ReadFailed {
            output: "HDMI-A-1".into(),
            code: 0xD6,
            detail: "checksum".into(),
        };
        assert_eq!(
            BackendError::from(read),
            BackendError::Io("reading VCP 0xd6 from HDMI-A-1 failed: checksum".into())
        );
        let timeout = DdcError::Timeout {
            op: "scan".into(),
            after: Duration::from_secs(3),
        };
        assert_eq!(
            BackendError::from(timeout),
            BackendError::Io("DDC/CI scan timed out after 3s".into())
        );
        assert_eq!(
            BackendError::from(DdcError::Unavailable("no buses".into())),
            BackendError::Unavailable("DDC/CI unavailable: no buses".into())
        );
    }

    #[test]
    fn only_reads_and_writes_are_retried() {
        let read = DdcError::ReadFailed {
            output: String::new(),
            code: 0,
            detail: String::new(),
        };
        assert!(read.is_retryable());
        assert!(!DdcError::Unavailable(String::new()).is_retryable());
        assert!(
            !DdcError::Timeout {
                op: String::new(),
                after: Duration::ZERO
            }
            .is_retryable()
        );
    }
}
