//! A non-consuming peek of the Wayland socket.
//!
//! libwayland's `wl_display_read_events` returns success on `EAGAIN`, and
//! `wl_display_get_protocol_error` does not include the compositor's message.
//! The bytes are still on the socket, so the pump looks at them itself.
//! `MSG_PEEK` leaves those bytes, and any file descriptors attached to them,
//! for the Wayland stack to read.

use std::os::fd::BorrowedFd;

use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::io::Errno;
use rustix::net::{RecvFlags, recv};

/// What a peek sees after [`wayland_client`]'s read returned.
///
/// [`wayland_client`]: https://docs.rs/wayland-client
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum AfterRead {
    /// No bytes left. Reactor readiness from this wake is stale.
    Idle,
    /// The peer closed the socket.
    Closed,
    /// More bytes are waiting.
    Pending,
}

/// Polls `fd` after a read. `poll` does not consume bytes or file descriptors.
pub(crate) fn after_read(fd: BorrowedFd<'_>) -> AfterRead {
    let mut fds = [PollFd::new(
        &fd,
        PollFlags::IN | PollFlags::ERR | PollFlags::HUP,
    )];
    let zero = Timespec::default();
    if poll(&mut fds, Some(&zero)).is_err() {
        return AfterRead::Pending;
    }
    let revents = fds[0].revents();
    if revents.contains(PollFlags::IN) {
        AfterRead::Pending
    } else if revents.intersects(PollFlags::HUP | PollFlags::ERR) {
        AfterRead::Closed
    } else {
        AfterRead::Idle
    }
}

/// A `wl_display.error` string currently waiting on `fd`.
pub(crate) fn protocol_text(fd: BorrowedFd<'_>) -> Option<String> {
    match peek(fd) {
        Peek::Bytes(buf) => display_error_text(&buf),
        Peek::Empty | Peek::Closed => None,
    }
}

enum Peek {
    Empty,
    Closed,
    Bytes(Vec<u8>),
}

fn peek(fd: BorrowedFd<'_>) -> Peek {
    let mut buf = [0u8; 8192];
    let flags = RecvFlags::PEEK | RecvFlags::DONTWAIT;
    match recv(fd, &mut buf, flags) {
        Ok((n, _)) if n > 0 => Peek::Bytes(buf[..n].to_vec()),
        Ok(_) => Peek::Closed,
        Err(Errno::AGAIN | Errno::INTR) => Peek::Empty,
        // An unexpected peek error is not a reason to drop the wake.
        Err(_) => Peek::Bytes(Vec::new()),
    }
}

/// The first `wl_display.error` message string in a peeked buffer.
pub(crate) fn display_error_text(buf: &[u8]) -> Option<String> {
    let mut at = 0;
    while let Some(message) = message_at(buf, at) {
        if message.sender == 1
            && message.opcode == 0
            && let Some(text) = error_string(message.body)
        {
            return Some(text);
        }
        at = message.next;
    }
    None
}

struct WireMessage<'a> {
    sender: u32,
    opcode: u16,
    body: &'a [u8],
    next: usize,
}

fn message_at(buf: &[u8], at: usize) -> Option<WireMessage<'_>> {
    let header = buf.get(at..at + 8)?;
    let sender = u32::from_ne_bytes(header[0..4].try_into().ok()?);
    let word = u32::from_ne_bytes(header[4..8].try_into().ok()?);
    let opcode = u16::try_from(word & 0xffff).ok()?;
    let size = usize::from(u16::try_from(word >> 16).ok()?);
    if size < 8 || size % 4 != 0 {
        return None;
    }
    let end = at.checked_add(size)?;
    let body = buf.get(at + 8..end)?;
    Some(WireMessage {
        sender,
        opcode,
        body,
        next: end,
    })
}

/// `wl_display.error` arguments are object id, code, then the string.
fn error_string(body: &[u8]) -> Option<String> {
    let len = u32::from_ne_bytes(body.get(8..12)?.try_into().ok()?);
    let len = usize::try_from(len).ok()?;
    if len == 0 {
        return None;
    }
    let bytes = body.get(12..12 + len)?;
    let text = std::str::from_utf8(bytes.strip_suffix(&[0])?).ok()?;
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wayland::test_server::{Arg, encode};

    #[test]
    fn display_error_text_skips_earlier_events() {
        let mut buf = encode(2, 0, &[]);
        buf.extend(encode(
            1,
            0,
            &[Arg::Uint(5), Arg::Uint(0), Arg::Str("idled twice")],
        ));
        assert_eq!(display_error_text(&buf).as_deref(), Some("idled twice"));
    }

    #[test]
    fn a_truncated_buffer_is_not_a_message() {
        assert_eq!(display_error_text(&[1, 0, 0, 0, 8]), None);
    }

    #[test]
    fn an_empty_error_string_is_ignored() {
        // `wl_display.error` with object 5, code 0, and a zero-length string.
        let message = [1, 0, 0, 0, 0, 0, 20, 0, 5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(display_error_text(&message), None);
    }
}
