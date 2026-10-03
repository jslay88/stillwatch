//! Seams over udev and evdev, so hotplug and device handling run against
//! scripted fakes in tests.

use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};

use stillwatch_core::backend::BackendError;
use tokio::sync::mpsc;

use super::axis::{Axes, PadInput};

/// A joystick event node appeared or went away.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Hotplug {
    /// A joystick node was added or changed and may be openable now.
    Added(PathBuf),
    /// An input event node was removed.
    Removed(PathBuf),
}

/// Hotplug changes as they happen. An `Err` item, or the sender going away,
/// means the monitor died.
#[derive(Debug)]
pub(crate) struct HotplugReceiver(mpsc::UnboundedReceiver<Result<Hotplug, BackendError>>);

impl HotplugReceiver {
    pub(crate) fn new(rx: mpsc::UnboundedReceiver<Result<Hotplug, BackendError>>) -> Self {
        Self(rx)
    }

    /// The next change. Cancel-safe.
    pub(crate) async fn next_change(&mut self) -> Result<Hotplug, BackendError> {
        self.0
            .recv()
            .await
            .unwrap_or_else(|| Err(BackendError::Disconnected("udev monitor stopped".into())))
    }
}

/// Where gamepads come from.
pub(crate) trait Platform: Send + Sync + 'static {
    /// One open device.
    type Pad: Pad;

    /// Starts listening for hotplug. Called before [`joysticks`](Self::joysticks)
    /// so a pad plugged in between isn't missed.
    fn monitor(&self) -> impl Future<Output = Result<HotplugReceiver, BackendError>> + Send;

    /// The joystick event nodes present right now.
    fn joysticks(&self) -> Result<Vec<PathBuf>, BackendError>;

    /// Opens one event node for reading.
    fn open(&self, node: &Path) -> io::Result<Self::Pad>;
}

/// One open gamepad's event stream.
pub(crate) trait Pad: Send + 'static {
    /// The name the device reports.
    fn name(&self) -> &str;

    /// Every key and button code the device declares.
    fn keys(&self) -> &[u16];

    /// The absolute axes, as they were when the device was opened.
    fn axes(&self) -> Axes;

    /// Waits for the next event. An error ends the device (usually unplugged).
    fn next_input(&mut self) -> impl Future<Output = io::Result<PadInput>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn receiver_yields_changes_then_reports_a_dead_monitor() {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut changes = HotplugReceiver::new(rx);
        let node = PathBuf::from("/dev/input/event9");
        tx.send(Ok(Hotplug::Added(node.clone()))).unwrap();
        assert_eq!(changes.next_change().await, Ok(Hotplug::Added(node)));
        drop(tx);
        assert!(matches!(
            changes.next_change().await,
            Err(BackendError::Disconnected(_))
        ));
    }
}
