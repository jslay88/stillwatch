//! Shared plumbing for Wayland backends: an async event pump over a
//! [`Connection`] and the mapping from Wayland errors to [`BackendError`].

use std::io::ErrorKind;
use std::os::fd::{AsFd as _, OwnedFd};

use stillwatch_core::backend::BackendError;
use tokio::io::unix::AsyncFd;
use wayland_client::backend::WaylandError;
use wayland_client::{ConnectError, Connection, DispatchError, EventQueue, QueueHandle};

/// Drives one event queue from the tokio reactor, without polling.
///
/// Waits for the connection's socket to become readable, reads, and
/// dispatches into the caller's state. Zero CPU while the compositor is quiet.
pub struct EventPump<S> {
    queue: EventQueue<S>,
    socket: AsyncFd<OwnedFd>,
}

impl<S: 'static> EventPump<S> {
    /// A pump with a fresh event queue on `conn`.
    ///
    /// # Errors
    ///
    /// Fails outside a tokio runtime with I/O enabled, or if the socket can't
    /// be duplicated for the reactor.
    pub fn new(conn: &Connection) -> Result<Self, BackendError> {
        let fd = conn.as_fd().try_clone_to_owned()?;
        Ok(Self {
            queue: conn.new_event_queue(),
            socket: AsyncFd::new(fd)?,
        })
    }

    /// The handle new protocol objects are created on.
    #[must_use]
    pub fn handle(&self) -> QueueHandle<S> {
        self.queue.handle()
    }

    /// Dispatches whatever is queued, flushes requests, then waits for and
    /// reads the next batch of events. Call it in a loop.
    ///
    /// # Errors
    ///
    /// [`BackendError::Disconnected`] when the socket fails or closes,
    /// [`BackendError::Protocol`] on a protocol error.
    pub async fn turn(&mut self, state: &mut S) -> Result<(), BackendError> {
        self.queue.dispatch_pending(state).map_err(dispatch_error)?;
        match self.queue.flush() {
            Err(error) if !is_would_block(&error) => return Err(wayland_error(error)),
            _ => {}
        }
        let Some(read) = self.queue.prepare_read() else {
            return Ok(());
        };
        let mut ready = self.socket.readable().await?;
        match read.read() {
            Ok(_) => Ok(()),
            Err(error) if is_would_block(&error) => {
                ready.clear_ready();
                Ok(())
            }
            Err(error) => {
                // A read that hits EOF may already have queued the compositor's
                // last events; deliver them before reporting the loss.
                let _ = self.queue.dispatch_pending(state);
                Err(wayland_error(error))
            }
        }
    }

    /// Turns the pump until `done(state)` holds.
    ///
    /// # Errors
    ///
    /// As [`turn`](Self::turn).
    pub async fn run_until(
        &mut self,
        state: &mut S,
        done: impl Fn(&S) -> bool,
    ) -> Result<(), BackendError> {
        loop {
            self.queue.dispatch_pending(state).map_err(dispatch_error)?;
            if done(state) {
                return Ok(());
            }
            self.turn(state).await?;
        }
    }
}

fn is_would_block(error: &WaylandError) -> bool {
    matches!(error, WaylandError::Io(io) if io.kind() == ErrorKind::WouldBlock)
}

/// Maps a connection failure. A missing socket is treated as transient, since
/// the compositor may be restarting.
pub fn connect_error(error: &ConnectError) -> BackendError {
    match error {
        ConnectError::NoCompositor => {
            BackendError::Disconnected(format!("can't reach the Wayland compositor: {error}"))
        }
        _ => BackendError::Unavailable(format!("can't connect to Wayland: {error}")),
    }
}

/// Maps an error from reading or writing the connection.
pub fn wayland_error(error: WaylandError) -> BackendError {
    match error {
        WaylandError::Io(io) => {
            BackendError::Disconnected(format!("Wayland connection lost: {io}"))
        }
        WaylandError::Protocol(protocol) => BackendError::Protocol(protocol.to_string()),
    }
}

fn dispatch_error(error: DispatchError) -> BackendError {
    match error {
        DispatchError::Backend(backend) => wayland_error(backend),
        bad @ DispatchError::BadMessage { .. } => BackendError::Protocol(bad.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use wayland_client::backend::protocol::ProtocolError;

    use super::*;

    #[test]
    fn connect_errors_split_into_transient_and_permanent() {
        assert!(connect_error(&ConnectError::NoCompositor).is_transient());
        let garbage = connect_error(&ConnectError::InvalidFd);
        assert!(matches!(garbage, BackendError::Unavailable(_)), "{garbage}");
    }

    #[test]
    fn socket_errors_are_disconnects_and_protocol_errors_are_not() {
        let reset = WaylandError::Io(io::Error::from(ErrorKind::ConnectionReset));
        assert!(!is_would_block(&reset));
        assert!(matches!(
            wayland_error(reset),
            BackendError::Disconnected(_)
        ));

        let protocol = WaylandError::Protocol(ProtocolError {
            code: 3,
            object_id: 7,
            object_interface: "ext_idle_notifier_v1".into(),
            message: "bad seat".into(),
        });
        let mapped = dispatch_error(DispatchError::Backend(protocol));
        assert!(
            matches!(&mapped, BackendError::Protocol(m) if m.contains("bad seat")),
            "{mapped}"
        );
    }

    #[test]
    fn would_block_is_recognized() {
        let blocked = WaylandError::Io(io::Error::from(ErrorKind::WouldBlock));
        assert!(is_would_block(&blocked));
    }

    #[test]
    fn bad_messages_are_protocol_errors() {
        let bad = DispatchError::BadMessage {
            sender_id: wayland_client::backend::ObjectId::null(),
            interface: "wl_registry",
            opcode: 9,
        };
        assert!(matches!(dispatch_error(bad), BackendError::Protocol(_)));
    }
}
