use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use super::*;

fn exe() -> PathBuf {
    PathBuf::from("/usr/bin/stillwatchd")
}

fn by_name(name: &str) -> BackendError {
    method_error(name, Some("details"), "HDMI-A-1", &exe())
}

#[test]
fn kwin_and_bus_refusals_are_permission_denied_with_remediation() {
    for name in [NOT_AUTHORIZED, ACCESS_DENIED] {
        let BackendError::PermissionDenied(message) = by_name(name) else {
            panic!("{name} wasn't PermissionDenied");
        };
        assert!(message.contains(DESKTOP_FILE), "{message}");
        assert!(
            message.contains("~/.local/share/applications/"),
            "{message}"
        );
        assert!(message.contains("Exec=/usr/bin/stillwatchd"), "{message}");
        assert!(
            message.contains("X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2"),
            "{message}"
        );
    }
}

#[test]
fn a_replaced_binary_says_to_restart() {
    let message = not_authorized(Path::new("/usr/bin/stillwatchd (deleted)"));
    assert!(
        message.contains("/usr/bin/stillwatchd was replaced"),
        "{message}"
    );
    assert!(message.ends_with("Restart stillwatchd"), "{message}");
}

#[test]
fn an_unknown_output_is_not_found() {
    assert_eq!(
        by_name(INVALID_SCREEN),
        BackendError::NotFound("KWin has no output named \"HDMI-A-1\"".into())
    );
}

#[test]
fn render_and_pipe_failures_are_transient() {
    for name in [CANCELLED, FILE_DESCRIPTOR] {
        let err = by_name(name);
        assert!(err.is_transient(), "{err}");
        assert!(err.to_string().contains("details"), "{err}");
    }
}

#[test]
fn a_missing_service_or_interface_is_unavailable() {
    for name in MISSING {
        assert_eq!(
            by_name(name),
            BackendError::Unavailable("KWin ScreenShot2 isn't available: details".into())
        );
    }
}

#[test]
fn other_errors_keep_their_name() {
    assert_eq!(
        method_error("org.example.Weird", None, "DP-1", &exe()),
        BackendError::Protocol("ScreenShot2 failed with org.example.Weird: no details".into())
    );
}

#[test]
fn fdo_errors_map_by_name() {
    let fdo = zbus::fdo::Error::AccessDenied("nope".into());
    let err = call_error(zbus::Error::FDO(Box::new(fdo)), "DP-1", &exe());
    assert!(matches!(err, BackendError::PermissionDenied(_)), "{err}");

    let fdo = zbus::fdo::Error::ServiceUnknown("gone".into());
    let err = call_error(zbus::Error::FDO(Box::new(fdo)), "DP-1", &exe());
    assert_eq!(
        err,
        BackendError::Unavailable("KWin ScreenShot2 isn't available: gone".into())
    );
}

#[test]
fn transport_errors_map_to_backend_errors() {
    let lost = zbus::Error::InputOutput(Arc::new(io::Error::from(ErrorKind::BrokenPipe)));
    assert!(matches!(
        call_error(lost, "DP-1", &exe()),
        BackendError::Disconnected(_)
    ));
    let slow = zbus::Error::InputOutput(Arc::new(io::Error::from(ErrorKind::TimedOut)));
    let err = call_error(slow, "DP-1", &exe());
    assert!(
        matches!(&err, BackendError::Io(m) if m.starts_with("KWin didn't answer")),
        "{err}"
    );
    assert!(matches!(
        call_error(zbus::Error::InterfaceNotFound, "DP-1", &exe()),
        BackendError::Unavailable(_)
    ));
    assert!(matches!(
        call_error(zbus::Error::InvalidReply, "DP-1", &exe()),
        BackendError::Protocol(_)
    ));
}
