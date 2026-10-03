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

use wayland_client::protocol::wl_output::{self, Mode, Transform, WlOutput};
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::{Connection, Dispatch, Proxy as _, QueueHandle, WEnum};

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
    width: u32,
    height: u32,
    has_size: bool,
    rotated: bool,
}

/// A named output and the size of its current mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedOutput {
    /// Registry id of the `wl_output` global.
    pub global: OutputGlobal,
    /// Connector name.
    pub name: String,
    /// Width in physical pixels, swapped with height when rotated.
    pub width: u32,
    /// Height in physical pixels, swapped with width when rotated.
    pub height: u32,
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
                    width: 0,
                    height: 0,
                    has_size: false,
                    rotated: false,
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

    /// Named outputs that already have a current mode.
    #[must_use]
    pub fn named(&self) -> Vec<NamedOutput> {
        self.outputs
            .iter()
            .filter_map(|output| {
                let name = output.name.clone()?;
                if !output.has_size || output.width == 0 || output.height == 0 {
                    return None;
                }
                let (width, height) = if output.rotated {
                    (output.height, output.width)
                } else {
                    (output.width, output.height)
                };
                Some(NamedOutput {
                    global: output.global,
                    name,
                    width,
                    height,
                })
            })
            .collect()
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
            wl_output::Event::Mode {
                flags: WEnum::Value(flags),
                width,
                height,
                ..
            } if flags.contains(Mode::Current) => {
                if let (Ok(width), Ok(height)) = (u32::try_from(width), u32::try_from(height)) {
                    output.width = width;
                    output.height = height;
                    output.has_size = true;
                }
            }
            wl_output::Event::Geometry {
                transform: WEnum::Value(transform),
                ..
            } => {
                output.rotated = matches!(
                    transform,
                    Transform::_90 | Transform::_270 | Transform::Flipped90 | Transform::Flipped270
                );
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
