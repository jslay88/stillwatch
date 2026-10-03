//! The hotplug loop and one forwarding task per open device.

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use stillwatch_core::backend::{BackendError, EventSink, GamepadDevice};
use stillwatch_core::event::ActivityEvent;
use stillwatch_core::time::Clock;
use tokio::task::{self, AbortHandle, JoinError, JoinSet};

use super::axis::{Axes, PadInput};
use super::platform::{Hotplug, Pad, Platform};
use super::registry::Registry;
use super::settings::GamepadSettings;

/// State shared between the source, the hotplug loop, and device tasks.
pub(crate) struct Shared {
    registry: Mutex<Registry>,
    clock: Arc<dyn Clock>,
}

impl Shared {
    pub(crate) fn new(registry: Registry, clock: Arc<dyn Clock>) -> Self {
        Self {
            registry: Mutex::new(registry),
            clock,
        }
    }

    fn registry(&self) -> MutexGuard<'_, Registry> {
        self.registry.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn apply(&self, settings: &GamepadSettings) {
        self.registry().apply(settings);
    }

    pub(crate) fn snapshot(&self) -> Vec<GamepadDevice> {
        self.registry().snapshot()
    }

    /// Whether `input` from `id` should produce an activity event now.
    fn accept(&self, id: &str, axes: &Axes, input: PadInput) -> bool {
        let mut registry = self.registry();
        axes.counts(input, registry.deadzone_percent()) && registry.record(id, self.clock.now())
    }
}

fn device_id(node: &Path) -> String {
    node.display().to_string()
}

/// Watches hotplug and forwards activity from every joystick until the
/// monitor fails. Dropping the future closes every device.
pub(crate) async fn run<P: Platform>(
    platform: &P,
    shared: &Arc<Shared>,
    sink: Arc<dyn EventSink>,
) -> Result<(), BackendError> {
    let mut changes = platform.monitor().await?;
    let mut pads = Pads::new(Arc::clone(shared), sink);
    for node in platform.joysticks()? {
        pads.open(platform, node);
    }
    loop {
        tokio::select! {
            change = changes.next_change() => match change? {
                Hotplug::Added(node) => pads.open(platform, node),
                Hotplug::Removed(node) => pads.close(&node),
            },
            Some(done) = pads.tasks.join_next_with_id() => pads.finished(&done),
        }
    }
}

struct Pads {
    shared: Arc<Shared>,
    sink: Arc<dyn EventSink>,
    tasks: JoinSet<()>,
    open: HashMap<PathBuf, AbortHandle>,
    denied: HashSet<PathBuf>,
}

impl Pads {
    fn new(shared: Arc<Shared>, sink: Arc<dyn EventSink>) -> Self {
        Self {
            shared,
            sink,
            tasks: JoinSet::new(),
            open: HashMap::new(),
            denied: HashSet::new(),
        }
    }

    fn open<P: Platform>(&mut self, platform: &P, node: PathBuf) {
        if self.open.contains_key(&node) {
            return;
        }
        let pad = match platform.open(&node) {
            Ok(pad) => pad,
            Err(err) => {
                self.open_failed(node, &err);
                return;
            }
        };
        self.denied.remove(&node);
        let id = device_id(&node);
        let ignored = self
            .shared
            .registry()
            .insert(id.clone(), pad.name().to_owned());
        tracing::info!(device = %id, name = pad.name(), ignored, "gamepad connected");
        let handle = self.tasks.spawn(forward(
            id,
            pad,
            Arc::clone(&self.shared),
            Arc::clone(&self.sink),
        ));
        self.open.insert(node, handle);
    }

    fn open_failed(&mut self, node: PathBuf, err: &io::Error) {
        let device = device_id(&node);
        if err.kind() != io::ErrorKind::PermissionDenied {
            tracing::debug!(%device, %err, "can't open gamepad");
        } else if self.denied.insert(node) {
            tracing::warn!(
                %device,
                %err,
                "can't open gamepad; it needs the uaccess tag or the input group"
            );
        }
    }

    fn close(&mut self, node: &Path) {
        self.denied.remove(node);
        if let Some(handle) = self.open.get(node) {
            handle.abort();
            self.forget(node);
        }
    }

    fn finished(&mut self, done: &Result<(task::Id, ()), JoinError>) {
        let id = match done {
            Ok((id, ())) => *id,
            Err(err) => err.id(),
        };
        let node = self
            .open
            .iter()
            .find(|(_, handle)| handle.id() == id)
            .map(|(node, _)| node.clone());
        if let Some(node) = node {
            self.forget(&node);
        }
    }

    fn forget(&mut self, node: &Path) {
        self.open.remove(node);
        let device = device_id(node);
        self.shared.registry().remove(&device);
        tracing::info!(%device, "gamepad disconnected");
    }
}

impl Drop for Pads {
    fn drop(&mut self) {
        let mut registry = self.shared.registry();
        for node in self.open.keys() {
            registry.remove(&device_id(node));
        }
    }
}

async fn forward<P: Pad>(id: String, mut pad: P, shared: Arc<Shared>, sink: Arc<dyn EventSink>) {
    let axes = pad.axes();
    loop {
        match pad.next_input().await {
            Ok(input) => {
                if shared.accept(&id, &axes, input) {
                    sink.send(ActivityEvent::GamepadActivity { device: id.clone() }.into());
                }
            }
            Err(err) => {
                tracing::debug!(device = %id, %err, "gamepad stream ended");
                return;
            }
        }
    }
}
