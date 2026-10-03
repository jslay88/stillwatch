//! The real [`Platform`]: udev for discovery and hotplug, evdev for input.

use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::thread;

use futures_util::{Stream, StreamExt as _};
use stillwatch_core::backend::BackendError;
use tokio::sync::{mpsc, oneshot};
use tokio_udev::{AsyncMonitorSocket, Device, Enumerator, EventType, MonitorBuilder};

use super::evdev_pad::EvdevPad;
use super::platform::{Hotplug, HotplugReceiver, Platform};

const SUBSYSTEM: &str = "input";
const JOYSTICK: &str = "ID_INPUT_JOYSTICK";

type ChangeSender = mpsc::UnboundedSender<Result<Hotplug, BackendError>>;

/// udev and evdev on the running system.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct UdevPlatform;

impl Platform for UdevPlatform {
    type Pad = EvdevPad;

    async fn monitor(&self) -> Result<HotplugReceiver, BackendError> {
        let (ready_tx, ready_rx) = oneshot::channel();
        let (tx, rx) = mpsc::unbounded_channel();
        thread::Builder::new()
            .name("stillwatch-udev".into())
            .spawn(move || monitor_thread(ready_tx, &tx))?;
        ready_rx
            .await
            .map_err(|_| BackendError::Unavailable("udev monitor thread exited".into()))??;
        Ok(HotplugReceiver::new(rx))
    }

    fn joysticks(&self) -> Result<Vec<PathBuf>, BackendError> {
        let mut enumerator = Enumerator::new()?;
        enumerator.match_subsystem(SUBSYSTEM)?;
        enumerator.match_property(JOYSTICK, "1")?;
        let mut nodes: Vec<PathBuf> = enumerator
            .scan_devices()?
            .filter_map(|device| event_node(device.devnode()).map(Path::to_path_buf))
            .collect();
        nodes.sort();
        Ok(nodes)
    }

    fn open(&self, node: &Path) -> io::Result<EvdevPad> {
        EvdevPad::open(node)
    }
}

/// The udev socket isn't `Send`, so it lives on its own thread with a
/// single-threaded runtime and hands changes over a channel. The thread exits
/// when the receiving side is dropped.
fn monitor_thread(ready: oneshot::Sender<Result<(), BackendError>>, tx: &ChangeSender) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            let _ = ready.send(Err(err.into()));
            return;
        }
    };
    runtime.block_on(async move {
        let socket = match listen() {
            Ok(socket) => socket,
            Err(err) => {
                let _ = ready.send(Err(err));
                return;
            }
        };
        let _ = ready.send(Ok(()));
        let changes = socket.map(|event| event.map(|event| change_for(event.event_type(), &event)));
        forward_changes(changes, tx).await;
    });
}

fn listen() -> Result<AsyncMonitorSocket, BackendError> {
    let socket = MonitorBuilder::new()?
        .match_subsystem(SUBSYSTEM)?
        .listen()?;
    Ok(AsyncMonitorSocket::new(socket)?)
}

fn change_for(kind: EventType, device: &Device) -> Option<Hotplug> {
    let joystick = device
        .property_value(JOYSTICK)
        .is_some_and(|value| value == "1");
    hotplug(kind, device.devnode(), joystick)
}

/// Sends every relevant change until the stream ends or fails (reported as an
/// error item) or nobody is listening any more.
async fn forward_changes<S>(mut changes: S, tx: &ChangeSender)
where
    S: Stream<Item = io::Result<Option<Hotplug>>> + Unpin,
{
    loop {
        let item = tokio::select! {
            () = tx.closed() => return,
            item = changes.next() => item,
        };
        let change = match item {
            Some(Ok(Some(change))) => Ok(change),
            Some(Ok(None)) => continue,
            Some(Err(err)) => Err(BackendError::from(err)),
            None => Err(BackendError::Disconnected("udev monitor closed".into())),
        };
        let fatal = change.is_err();
        if tx.send(change).is_err() || fatal {
            return;
        }
    }
}

/// Only `/dev/input/event*` nodes carry evdev events; `js*` and `mouse*` are
/// legacy interfaces to the same device.
fn event_node(devnode: Option<&Path>) -> Option<&Path> {
    devnode.filter(|node| {
        node.file_name()
            .and_then(OsStr::to_str)
            .is_some_and(|name| name.starts_with("event"))
    })
}

/// Maps one udev event to a hotplug change. Removals don't check the joystick
/// tag, since removing an event node that was never opened is harmless.
fn hotplug(kind: EventType, devnode: Option<&Path>, joystick: bool) -> Option<Hotplug> {
    let node = event_node(devnode)?.to_path_buf();
    match kind {
        EventType::Add | EventType::Change if joystick => Some(Hotplug::Added(node)),
        EventType::Remove => Some(Hotplug::Removed(node)),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
