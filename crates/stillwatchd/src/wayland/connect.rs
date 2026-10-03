//! Connecting to the session compositor or to a named Wayland display.

use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use stillwatch_core::backend::BackendError;
use wayland_client::Connection;

use super::connect_error;

/// Connects to `display` (a socket name in `$XDG_RUNTIME_DIR`, or an
/// absolute path), or with `WAYLAND_DISPLAY` / `WAYLAND_SOCKET` when `None`.
///
/// # Errors
///
/// [`BackendError::Disconnected`] when nothing listens on the socket, which
/// may be a compositor restart. [`BackendError::Unavailable`] when a display
/// name can't be resolved without `XDG_RUNTIME_DIR`.
pub fn connect_to(display: Option<&str>) -> Result<Connection, BackendError> {
    let Some(display) = display else {
        return Connection::connect_to_env().map_err(|error| connect_error(&error));
    };
    let path = socket_path(
        display,
        std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from),
    )?;
    let stream = UnixStream::connect(&path).map_err(|error| {
        BackendError::Disconnected(format!(
            "can't reach the Wayland compositor at {}: {error}",
            path.display()
        ))
    })?;
    Connection::from_socket(stream).map_err(|error| connect_error(&error))
}

fn socket_path(display: &str, runtime_dir: Option<PathBuf>) -> Result<PathBuf, BackendError> {
    let display = PathBuf::from(display);
    if display.is_absolute() {
        return Ok(display);
    }
    runtime_dir.map(|dir| dir.join(&display)).ok_or_else(|| {
        BackendError::Unavailable(format!(
            "XDG_RUNTIME_DIR is unset, so Wayland display {} can't be found",
            display.display()
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_names_resolve_in_the_runtime_dir() {
        let runtime = Some(PathBuf::from("/run/user/1000"));
        assert_eq!(
            socket_path("wayland-1", runtime.clone()),
            Ok(PathBuf::from("/run/user/1000/wayland-1"))
        );
        assert_eq!(
            socket_path("/tmp/kwin.sock", runtime),
            Ok(PathBuf::from("/tmp/kwin.sock"))
        );
        assert!(matches!(
            socket_path("wayland-1", None),
            Err(BackendError::Unavailable(_))
        ));
    }

    #[test]
    fn a_dead_socket_is_a_disconnect() {
        let error = connect_to(Some("/nonexistent/stillwatch-test.sock")).unwrap_err();
        assert!(error.is_transient(), "{error}");
    }
}
