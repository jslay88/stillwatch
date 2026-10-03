//! Connecting to `KWin`'s socket and reading its globals.

use std::os::unix::net::UnixStream;
use std::path::Path;

use tokio::time::{Instant, timeout_at};
use wayland_client::globals::{Global, GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::{Connection, Dispatch, QueueHandle};

use crate::Error;

/// Opens a client connection to the compositor listening at `socket`.
pub fn connect(socket: &Path) -> Result<Connection, Error> {
    let stream = UnixStream::connect(socket)?;
    Connection::from_socket(stream).map_err(|err| Error::Wayland(err.to_string()))
}

struct Probe;

impl Dispatch<WlRegistry, GlobalListContents> for Probe {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

/// Every global the compositor at `socket` advertises, after one roundtrip.
///
/// Gives up at `deadline` rather than waiting forever on a compositor that
/// accepted the connection but never answers. The roundtrip blocks a thread,
/// which is released once that compositor is killed.
pub async fn globals(socket: &Path, deadline: Instant) -> Result<Vec<Global>, Error> {
    let conn = connect(socket)?;
    let roundtrip = tokio::task::spawn_blocking(move || {
        registry_queue_init::<Probe>(&conn).map(|(list, _queue)| list.contents().clone_list())
    });
    match timeout_at(deadline, roundtrip).await {
        Ok(Ok(Ok(globals))) => Ok(globals),
        Ok(Ok(Err(err))) => Err(Error::Wayland(err.to_string())),
        Ok(Err(join)) => Err(Error::Wayland(join.to_string())),
        Err(_) => Err(Error::Wayland(
            "the registry roundtrip timed out".to_owned(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::net::UnixListener;
    use std::time::Duration;

    use super::*;

    #[tokio::test]
    async fn a_silent_compositor_times_out() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("silent");
        let _listener = UnixListener::bind(&socket).unwrap();
        let deadline = Instant::now() + Duration::from_millis(50);
        let err = globals(&socket, deadline).await.unwrap_err();
        assert!(err.to_string().contains("timed out"), "{err}");
    }

    #[tokio::test]
    async fn a_closed_socket_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("closed");
        let listener = UnixListener::bind(&socket).unwrap();
        std::thread::spawn(move || drop(listener.accept()));
        let deadline = Instant::now() + Duration::from_secs(5);
        let err = globals(&socket, deadline).await.unwrap_err();
        assert!(matches!(err, Error::Wayland(_)), "{err}");
    }

    #[test]
    fn nothing_listening_is_an_io_error() {
        let dir = tempfile::tempdir().unwrap();
        let err = connect(&dir.path().join("none")).unwrap_err();
        assert!(matches!(err, Error::Io(_)), "{err}");
    }
}
