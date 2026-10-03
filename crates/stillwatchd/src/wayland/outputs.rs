//! Connector names for the compositor's `wl_output`s, for Wayland backends
//! that don't use smithay-client-toolkit's own output tracking.
//!
//! [`OutputRegistry`] binds every `wl_output` global at version 4, which
//! sends the connector name (`HDMI-A-1`), and forgets outputs when their
//! global is removed. A backend's dispatch state owns one, passes its
//! `wl_registry` events to [`OutputRegistry::handle_registry`], implements
//! `AsMut<OutputRegistry>`, and delegates the `wl_output` events:
//!
//! ```ignore
//! delegate_dispatch!(MyState: [WlOutput: OutputGlobal] => OutputRegistry);
//! ```

use wayland_client::protocol::wl_output::{self, WlOutput};
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::{Connection, Dispatch, Proxy as _, QueueHandle};

pub use crate::outputs::NAME_VERSION;

/// `wl_output.release` arrived in version 3; older objects can't be released.
const RELEASE_VERSION: u32 = 3;

/// User data on every `wl_output` bound by [`OutputRegistry`]: the registry
/// name of its global.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OutputGlobal(pub u32);

/// What a registry event changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputChange {
    /// A new output was bound. Its name follows in later `wl_output` events.
    Added(OutputGlobal, WlOutput),
    /// An output's global went away. Holds its connector name if it had one.
    Removed(OutputGlobal, Option<String>),
}

#[derive(Debug)]
struct Tracked {
    global: OutputGlobal,
    proxy: WlOutput,
    name: Option<String>,
    pending_name: Option<String>,
}

/// The outputs a connection knows about, with their connector names.
#[derive(Debug, Default)]
pub struct OutputRegistry {
    outputs: Vec<Tracked>,
}

impl OutputRegistry {
    /// Binds new `wl_output` globals and forgets removed ones. Other
    /// registry events are ignored and return `None`.
    pub fn handle_registry<S>(
        &mut self,
        registry: &WlRegistry,
        event: &wl_registry::Event,
        qh: &QueueHandle<S>,
    ) -> Option<OutputChange>
    where
        S: Dispatch<WlOutput, OutputGlobal> + 'static,
    {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } if interface == WlOutput::interface().name => {
                let global = OutputGlobal(*name);
                let version = (*version).min(NAME_VERSION);
                if version < NAME_VERSION {
                    tracing::warn!(
                        version,
                        "wl_output is older than v4 and has no connector name; ignoring it"
                    );
                }
                let proxy: WlOutput = registry.bind(*name, version, qh, global);
                self.outputs.push(Tracked {
                    global,
                    proxy: proxy.clone(),
                    name: None,
                    pending_name: None,
                });
                Some(OutputChange::Added(global, proxy))
            }
            wl_registry::Event::GlobalRemove { name } => {
                let index = self.outputs.iter().position(|o| o.global.0 == *name)?;
                let removed = self.outputs.remove(index);
                if removed.proxy.version() >= RELEASE_VERSION {
                    removed.proxy.release();
                }
                Some(OutputChange::Removed(removed.global, removed.name))
            }
            _ => None,
        }
    }

    /// The connector name of `global`, once the compositor has sent it.
    #[must_use]
    pub fn name(&self, global: OutputGlobal) -> Option<&str> {
        self.outputs
            .iter()
            .find(|o| o.global == global)
            .and_then(|o| o.name.as_deref())
    }

    /// Every bound output, named or not.
    pub fn bound(&self) -> impl Iterator<Item = (OutputGlobal, &WlOutput)> {
        self.outputs.iter().map(|o| (o.global, &o.proxy))
    }

    fn handle_output(&mut self, global: OutputGlobal, event: wl_output::Event) {
        let Some(output) = self.outputs.iter_mut().find(|o| o.global == global) else {
            return;
        };
        match event {
            wl_output::Event::Name { name } => output.pending_name = Some(name),
            wl_output::Event::Done => {
                if let Some(name) = output.pending_name.take() {
                    output.name = Some(name);
                }
            }
            _ => {}
        }
    }
}

impl<S> Dispatch<WlOutput, OutputGlobal, S> for OutputRegistry
where
    S: Dispatch<WlOutput, OutputGlobal> + AsMut<Self>,
{
    fn event(
        state: &mut S,
        _: &WlOutput,
        event: wl_output::Event,
        global: &OutputGlobal,
        _: &Connection,
        _: &QueueHandle<S>,
    ) {
        state.as_mut().handle_output(*global, event);
    }
}
