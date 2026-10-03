//! The pipe `KWin` writes the image into.

use std::io::ErrorKind;
use std::os::fd::OwnedFd;
use std::time::Duration;

use stillwatch_core::backend::BackendError;
use tokio::io::AsyncReadExt as _;
use tokio::net::unix::pipe::{self, Receiver};

/// How long to wait for the whole image once `KWin` has replied. `KWin`
/// renders before replying and writes from a worker thread right after, so this only
/// trips when the writer is stuck.
pub const READ_TIMEOUT: Duration = Duration::from_secs(10);

/// A fresh pipe: the async read end, and the write end to hand to `KWin`.
///
/// # Errors
///
/// [`BackendError::Io`] if the pipe can't be created or registered with the
/// tokio reactor.
pub fn open() -> Result<(Receiver, OwnedFd), BackendError> {
    let (sender, receiver) = pipe::pipe()?;
    Ok((receiver, sender.into_blocking_fd()?))
}

/// Reads exactly `len` bytes.
///
/// Stopping at `len` instead of reading to EOF means a write end left open
/// somewhere (our message's copy, a server that doesn't close) can't stall
/// the capture.
///
/// # Errors
///
/// [`BackendError::Protocol`] when the writer closes early,
/// [`BackendError::Io`] on a read error or after [`READ_TIMEOUT`].
pub async fn read_frame(receiver: &mut Receiver, len: usize) -> Result<Vec<u8>, BackendError> {
    let mut data = vec![0; len];
    match tokio::time::timeout(READ_TIMEOUT, receiver.read_exact(&mut data)).await {
        Ok(Ok(_)) => Ok(data),
        Ok(Err(err)) if err.kind() == ErrorKind::UnexpectedEof => Err(BackendError::Protocol(
            format!("the capture pipe closed before all {len} bytes of the frame arrived"),
        )),
        Ok(Err(err)) => Err(err.into()),
        Err(_) => Err(BackendError::Io(format!(
            "timed out after {READ_TIMEOUT:?} waiting for the frame"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::Write as _;

    use super::*;

    #[tokio::test]
    async fn reads_exactly_the_frame_even_with_the_writer_open() {
        let (mut receiver, writer) = open().unwrap();
        let mut writer = File::from(writer);
        writer.write_all(&[1, 2, 3, 4, 5]).unwrap();
        assert_eq!(read_frame(&mut receiver, 4).await.unwrap(), [1, 2, 3, 4]);
    }

    #[tokio::test]
    async fn an_early_close_is_a_protocol_error() {
        let (mut receiver, writer) = open().unwrap();
        let mut writer = File::from(writer);
        writer.write_all(&[1, 2]).unwrap();
        drop(writer);
        let err = read_frame(&mut receiver, 4).await.unwrap_err();
        assert_eq!(
            err,
            BackendError::Protocol(
                "the capture pipe closed before all 4 bytes of the frame arrived".into()
            )
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_stuck_writer_times_out() {
        let (mut receiver, _writer) = open().unwrap();
        let err = read_frame(&mut receiver, 4).await.unwrap_err();
        assert!(
            matches!(&err, BackendError::Io(m) if m.contains("timed out")),
            "{err}"
        );
    }

    #[tokio::test]
    async fn an_empty_frame_reads_nothing() {
        let (mut receiver, _writer) = open().unwrap();
        assert_eq!(
            read_frame(&mut receiver, 0).await.unwrap(),
            Vec::<u8>::new()
        );
    }
}
