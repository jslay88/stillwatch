//! A just-enough Wayland compositor speaking the wire protocol over a socket
//! pair, so the real client code runs without a real compositor. Backends'
//! tests add their protocol's requests and events on top.

use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;

/// The `wl_display` singleton's object id.
pub const DISPLAY: u32 = 1;
/// `wl_display.sync`.
pub const DISPLAY_SYNC: u16 = 0;
const DISPLAY_GET_REGISTRY: u16 = 1;
const DISPLAY_ERROR: u16 = 0;
const DISPLAY_DELETE_ID: u16 = 1;
/// `wl_registry.bind`.
pub const REGISTRY_BIND: u16 = 0;
const REGISTRY_GLOBAL: u16 = 0;
const REGISTRY_GLOBAL_REMOVE: u16 = 1;
const CALLBACK_DONE: u16 = 0;

/// One argument of an event we send.
pub enum Arg<'a> {
    /// `uint` or `object`.
    Uint(u32),
    /// `string`.
    Str(&'a str),
}

/// A request the client sent.
pub struct Request {
    /// The object it was sent to.
    pub object: u32,
    /// Its opcode on that object's interface.
    pub opcode: u16,
    body: Vec<u8>,
    at: usize,
}

impl Request {
    /// Reads the next `uint`, `object`, or `new_id` argument.
    pub fn uint(&mut self) -> u32 {
        let bytes = self.body[self.at..self.at + 4].try_into().unwrap();
        self.at += 4;
        u32::from_ne_bytes(bytes)
    }

    /// Reads the next `string` argument.
    pub fn string(&mut self) -> String {
        let len = self.uint() as usize;
        let text = String::from_utf8(self.body[self.at..self.at + len - 1].to_vec()).unwrap();
        self.at += len.next_multiple_of(4);
        text
    }
}

/// A `wl_registry.bind` the client sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bind {
    /// The global's registry name.
    pub name: u32,
    /// Its interface.
    pub interface: String,
    /// The version asked for.
    pub version: u32,
    /// The object id the client allocated.
    pub id: u32,
}

impl Bind {
    /// Decodes a `wl_registry.bind` request.
    pub fn parse(mut request: Request) -> Self {
        Self {
            name: request.uint(),
            interface: request.string(),
            version: request.uint(),
            id: request.uint(),
        }
    }
}

/// The compositor end of a client connection.
pub struct FakeCompositor {
    stream: UnixStream,
    registry: u32,
}

impl FakeCompositor {
    /// The compositor end and the client's socket.
    pub fn pair() -> (Self, UnixStream) {
        let (server, client) = UnixStream::pair().unwrap();
        (Self::over(server), client)
    }

    const fn over(stream: UnixStream) -> Self {
        Self {
            stream,
            registry: 0,
        }
    }

    /// The client's `wl_registry` id, once [`advertise`](Self::advertise)
    /// has seen it.
    pub const fn registry(&self) -> u32 {
        self.registry
    }

    /// Reads the next request.
    pub fn read(&mut self) -> io::Result<Request> {
        let mut header = [0; 8];
        self.stream.read_exact(&mut header)?;
        let object = u32::from_ne_bytes(header[..4].try_into().unwrap());
        let word = u32::from_ne_bytes(header[4..].try_into().unwrap());
        let mut body = vec![0; (word >> 16) as usize - 8];
        self.stream.read_exact(&mut body)?;
        Ok(Request {
            object,
            opcode: (word & 0xffff) as u16,
            body,
            at: 0,
        })
    }

    /// Reads requests until one for `object` with `opcode` arrives, dropping
    /// the others.
    pub fn expect(&mut self, object: u32, opcode: u16) -> io::Result<Request> {
        loop {
            let request = self.read()?;
            if request.object == object && request.opcode == opcode {
                return Ok(request);
            }
        }
    }

    /// Sends an event.
    pub fn send(&mut self, object: u32, opcode: u16, args: &[Arg<'_>]) -> io::Result<()> {
        self.stream.write_all(&encode(object, opcode, args))
    }

    /// Answers the client's registry request and sync with `globals`
    /// (`(name, interface, version)`).
    pub fn advertise(&mut self, globals: &[(u32, &str, u32)]) -> io::Result<()> {
        self.advertise_then_remove(globals, &[])
    }

    /// Like [`advertise`](Self::advertise), but withdraws the `removed`
    /// global names again before the sync completes.
    pub fn advertise_then_remove(
        &mut self,
        globals: &[(u32, &str, u32)],
        removed: &[u32],
    ) -> io::Result<()> {
        self.registry = self.expect(DISPLAY, DISPLAY_GET_REGISTRY)?.uint();
        let callback = self.expect(DISPLAY, DISPLAY_SYNC)?.uint();
        for &(name, interface, version) in globals {
            if client_left(self.global(name, interface, version))? {
                return Ok(());
            }
        }
        for &name in removed {
            if client_left(self.remove_global(name))? {
                return Ok(());
            }
        }
        client_left(self.done(callback))?;
        Ok(())
    }

    /// Announces a global on the client's registry.
    pub fn global(&mut self, name: u32, interface: &str, version: u32) -> io::Result<()> {
        self.send(
            self.registry,
            REGISTRY_GLOBAL,
            &[Arg::Uint(name), Arg::Str(interface), Arg::Uint(version)],
        )
    }

    /// Withdraws a global from the client's registry.
    pub fn remove_global(&mut self, name: u32) -> io::Result<()> {
        self.send(self.registry, REGISTRY_GLOBAL_REMOVE, &[Arg::Uint(name)])
    }

    /// Answers a `wl_display.sync` whose callback is `callback`.
    pub fn done(&mut self, callback: u32) -> io::Result<()> {
        // A client may hang up as soon as it reads `done`, so `delete_id` has
        // to be in the same write or it can hit a closed socket.
        let mut reply = encode(callback, CALLBACK_DONE, &[Arg::Uint(0)]);
        reply.extend(encode(DISPLAY, DISPLAY_DELETE_ID, &[Arg::Uint(callback)]));
        self.stream.write_all(&reply)
    }

    /// Sends a fatal `wl_display.error` about `object`.
    pub fn protocol_error(&mut self, object: u32, message: &str) -> io::Result<()> {
        self.send(
            DISPLAY,
            DISPLAY_ERROR,
            &[Arg::Uint(object), Arg::Uint(0), Arg::Str(message)],
        )
    }

    /// Blocks until the client closes its end.
    pub fn wait_for_close(&mut self) {
        while self.read().is_ok() {}
    }
}

/// `true` when the client already hung up. A roundtrip that finds nothing to
/// bind (no idle notifier) closes the socket as soon as it has read `done`,
/// which races the rest of the reply. That is the client finishing, not a
/// compositor failure.
fn client_left(result: io::Result<()>) -> io::Result<bool> {
    match result {
        Ok(()) => Ok(false),
        Err(err)
            if matches!(
                err.kind(),
                io::ErrorKind::BrokenPipe
                    | io::ErrorKind::ConnectionReset
                    | io::ErrorKind::ConnectionAborted
            ) =>
        {
            Ok(true)
        }
        Err(err) => Err(err),
    }
}

/// One event on the wire.
pub(crate) fn encode(object: u32, opcode: u16, args: &[Arg<'_>]) -> Vec<u8> {
    let mut body = Vec::new();
    for arg in args {
        match arg {
            Arg::Uint(value) => body.extend(value.to_ne_bytes()),
            Arg::Str(text) => {
                let len = u32::try_from(text.len() + 1).unwrap();
                body.extend(len.to_ne_bytes());
                body.extend(text.as_bytes());
                body.push(0);
                body.resize(body.len().next_multiple_of(4), 0);
            }
        }
    }
    let size = u32::try_from(body.len() + 8).unwrap();
    let mut message = Vec::with_capacity(body.len() + 8);
    message.extend(object.to_ne_bytes());
    message.extend(((size << 16) | u32::from(opcode)).to_ne_bytes());
    message.extend(body);
    message
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::os::fd::OwnedFd;
    use std::os::unix::net::UnixDatagram;

    use super::*;

    #[test]
    fn a_sync_is_answered_in_one_write() {
        // Datagrams keep write boundaries, so one recv gets exactly one write.
        let (server, client) = UnixDatagram::pair().unwrap();
        let mut compositor = FakeCompositor::over(UnixStream::from(OwnedFd::from(server)));
        compositor.done(7).unwrap();

        let mut reply = [0; 64];
        assert_eq!(client.recv(&mut reply).unwrap(), 24);
        let object = |at: usize| u32::from_ne_bytes(reply[at..at + 4].try_into().unwrap());
        assert_eq!((object(0), object(12)), (7, DISPLAY));
    }

    #[test]
    fn a_client_that_leaves_during_the_reply_is_not_an_error() {
        let (mut server, mut client) = FakeCompositor::pair();
        client
            .write_all(&request(DISPLAY, DISPLAY_GET_REGISTRY, 2))
            .unwrap();
        client
            .write_all(&request(DISPLAY, DISPLAY_SYNC, 3))
            .unwrap();
        drop(client);

        server.advertise(&[(11, "wl_seat", 10)]).unwrap();
    }

    fn request(object: u32, opcode: u16, new_id: u32) -> [u8; 12] {
        let mut message = [0; 12];
        message[..4].copy_from_slice(&object.to_ne_bytes());
        message[4..8].copy_from_slice(&((12u32 << 16) | u32::from(opcode)).to_ne_bytes());
        message[8..].copy_from_slice(&new_id.to_ne_bytes());
        message
    }
}
