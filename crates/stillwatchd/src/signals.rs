//! Unix signal handling: SIGTERM and SIGINT stop the daemon, SIGHUP asks for
//! a config reload.

use std::io;

use tokio::signal::unix::{self, SignalKind};

/// A signal the daemon reacts to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// SIGTERM: stop cleanly (systemd's stop).
    Terminate,
    /// SIGINT: stop cleanly (Ctrl+C).
    Interrupt,
    /// SIGHUP: reload the config (systemd's `ExecReload`).
    Hangup,
}

/// Something that yields signals; `None` means no more will arrive.
pub trait SignalSource {
    /// Waits for the next signal.
    fn recv(&mut self) -> impl Future<Output = Option<Signal>>;
}

/// The process's real SIGTERM, SIGINT, and SIGHUP streams.
#[derive(Debug)]
pub struct Signals {
    terminate: unix::Signal,
    interrupt: unix::Signal,
    hangup: unix::Signal,
}

impl Signals {
    /// Installs handlers for SIGTERM, SIGINT, and SIGHUP. From here on those
    /// signals no longer kill the process.
    ///
    /// # Errors
    ///
    /// Fails if a handler can't be registered.
    pub fn install() -> io::Result<Self> {
        Ok(Self {
            terminate: unix::signal(SignalKind::terminate())?,
            interrupt: unix::signal(SignalKind::interrupt())?,
            hangup: unix::signal(SignalKind::hangup())?,
        })
    }
}

impl SignalSource for Signals {
    async fn recv(&mut self) -> Option<Signal> {
        tokio::select! {
            got = self.terminate.recv() => got.map(|()| Signal::Terminate),
            got = self.interrupt.recv() => got.map(|()| Signal::Interrupt),
            got = self.hangup.recv() => got.map(|()| Signal::Hangup),
        }
    }
}

/// Handles signals until one asks the daemon to stop, and returns it.
///
/// A closed source counts as [`Signal::Terminate`].
pub async fn wait_for_shutdown(source: &mut impl SignalSource) -> Signal {
    loop {
        match source.recv().await {
            Some(Signal::Hangup) => {
                tracing::info!("got SIGHUP, config reload isn't wired up yet");
            }
            Some(signal) => return signal,
            None => return Signal::Terminate,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;

    struct Scripted(VecDeque<Signal>);

    impl SignalSource for Scripted {
        fn recv(&mut self) -> impl Future<Output = Option<Signal>> {
            std::future::ready(self.0.pop_front())
        }
    }

    async fn shutdown_after(signals: &[Signal]) -> (Signal, usize) {
        let mut source = Scripted(signals.iter().copied().collect());
        let signal = wait_for_shutdown(&mut source).await;
        (signal, source.0.len())
    }

    #[tokio::test]
    async fn terminate_stops() {
        assert_eq!(
            shutdown_after(&[Signal::Terminate]).await,
            (Signal::Terminate, 0)
        );
    }

    #[tokio::test]
    async fn interrupt_stops() {
        assert_eq!(
            shutdown_after(&[Signal::Interrupt, Signal::Terminate]).await,
            (Signal::Interrupt, 1)
        );
    }

    #[tokio::test]
    async fn hangup_keeps_running() {
        assert_eq!(
            shutdown_after(&[Signal::Hangup, Signal::Hangup, Signal::Interrupt]).await,
            (Signal::Interrupt, 0)
        );
    }

    #[tokio::test]
    async fn closed_source_counts_as_terminate() {
        assert_eq!(
            shutdown_after(&[Signal::Hangup]).await,
            (Signal::Terminate, 0)
        );
    }
}
