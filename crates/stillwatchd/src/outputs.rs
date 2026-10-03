//! Connected outputs from the compositor's `wl_output` globals.
//!
//! `wl_output` v4 sends each output's `name`, which on `KWin` is the connector
//! name (`HDMI-A-1`) that `ScreenShot2` and `kscreen-doctor` take. The size is
//! the current mode, swapped for 90 and 270 degree transforms so it matches
//! what a native-resolution capture returns. Outputs below v4 have no name
//! and are skipped.

use stillwatch_core::backend::BackendError;
use stillwatch_core::luma::OutputInfo;
use wayland_client::protocol::wl_callback::{self, WlCallback};
use wayland_client::protocol::wl_output::{self, Mode, Transform, WlOutput};
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::{Connection, Dispatch, Proxy as _, QueueHandle, WEnum};

use crate::wayland::{EventPump, connect_error};

/// The `wl_output` version that added the `name` event.
pub const NAME_VERSION: u32 = 4;

/// Lists outputs on the compositor `WAYLAND_DISPLAY` points at.
///
/// # Errors
///
/// [`BackendError::Disconnected`] when no compositor is reachable, otherwise
/// whatever [`list_on`] returns.
pub async fn list() -> Result<Vec<OutputInfo>, BackendError> {
    Ok(infos(list_placed().await?))
}

/// Lists outputs, including compositor position and scale.
///
/// # Errors
///
/// As [`list`].
pub async fn list_placed() -> Result<Vec<PlacedOutput>, BackendError> {
    let conn = Connection::connect_to_env().map_err(|err| connect_error(&err))?;
    list_placed_on(&conn).await
}

/// Lists outputs on `conn`, in the order the compositor advertises them.
///
/// # Errors
///
/// [`BackendError::Disconnected`] or [`BackendError::Protocol`] if the
/// connection fails part way.
pub async fn list_on(conn: &Connection) -> Result<Vec<OutputInfo>, BackendError> {
    Ok(infos(list_placed_on(conn).await?))
}

/// [`list_placed`] on an existing connection.
///
/// # Errors
///
/// As [`list_on`].
pub async fn list_placed_on(conn: &Connection) -> Result<Vec<PlacedOutput>, BackendError> {
    let mut pump = EventPump::new(conn)?;
    let qh = pump.handle();
    let display = conn.display();
    let _registry = display.get_registry(&qh, ());
    display.sync(&qh, Round::Globals);
    let mut listing = Listing::default();
    pump.run_until(&mut listing, |listing| listing.done).await?;
    Ok(listing.finish())
}

fn infos(placed: Vec<PlacedOutput>) -> Vec<OutputInfo> {
    placed.into_iter().map(|output| output.info).collect()
}

/// Which `wl_display.sync` a callback answers. Outputs are bound while the
/// first round's globals arrive, and their events all precede the second
/// round's `done`.
#[derive(Debug, Clone, Copy)]
enum Round {
    Globals,
    Outputs,
}

#[derive(Default)]
struct Listing {
    outputs: Vec<OutputBuilder>,
    unnamed: u32,
    done: bool,
}

impl Listing {
    fn finish(self) -> Vec<PlacedOutput> {
        if self.unnamed > 0 {
            tracing::warn!(
                count = self.unnamed,
                "skipping wl_output globals older than v{NAME_VERSION} (no connector name)"
            );
        }
        self.outputs
            .into_iter()
            .filter_map(|output| {
                let placed = output.placed();
                if placed.is_none() {
                    tracing::warn!(?output, "skipping an output with no name or current mode");
                }
                placed
            })
            .collect()
    }
}

/// A connected output plus its place in the compositor's logical layout.
///
/// The portal reports each stream's position and size in that same space, so
/// [`x`](Self::x) and [`y`](Self::y) are how a stream is matched to a
/// connector. [`scale`](Self::scale) is the `wl_output` scale factor; the
/// logical size is the mode size divided by it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacedOutput {
    /// Connector name and current mode size, in physical pixels.
    pub info: OutputInfo,
    /// Left edge in compositor coordinates.
    pub x: i32,
    /// Top edge in compositor coordinates.
    pub y: i32,
    /// `wl_output` scale, at least 1.
    pub scale: i32,
}

/// One output's events, gathered until the listing is complete.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct OutputBuilder {
    name: Option<String>,
    mode: Option<(u32, u32)>,
    rotated: bool,
    x: i32,
    y: i32,
    /// Zero until a `scale` event arrives.
    scale: i32,
}

impl OutputBuilder {
    /// Folds in one `wl_output` event.
    pub fn apply(&mut self, event: wl_output::Event) {
        match event {
            wl_output::Event::Name { name } => self.name = Some(name),
            wl_output::Event::Mode {
                flags: WEnum::Value(flags),
                width,
                height,
                ..
            } if flags.contains(Mode::Current) => {
                self.mode = u32::try_from(width).ok().zip(u32::try_from(height).ok());
            }
            wl_output::Event::Geometry {
                x,
                y,
                transform: WEnum::Value(transform),
                ..
            } => {
                self.x = x;
                self.y = y;
                self.rotated = matches!(
                    transform,
                    Transform::_90 | Transform::_270 | Transform::Flipped90 | Transform::Flipped270
                );
            }
            wl_output::Event::Scale { factor } => self.scale = factor,
            _ => {}
        }
    }

    /// The output and its layout, once it has a name and a current mode.
    #[must_use]
    pub fn placed(&self) -> Option<PlacedOutput> {
        Some(PlacedOutput {
            info: self.build()?,
            x: self.x,
            y: self.y,
            scale: self.scale.max(1),
        })
    }

    /// The output, once it has a name and a current mode.
    #[must_use]
    pub fn build(&self) -> Option<OutputInfo> {
        let name = self.name.clone()?;
        let (width, height) = self.mode?;
        let (width, height) = if self.rotated {
            (height, width)
        } else {
            (width, height)
        };
        Some(OutputInfo::new(name, width, height))
    }
}

impl Dispatch<WlRegistry, ()> for Listing {
    fn event(
        listing: &mut Self,
        registry: &WlRegistry,
        event: wl_registry::Event,
        (): &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        else {
            return;
        };
        if interface != WlOutput::interface().name {
            return;
        }
        if version < NAME_VERSION {
            listing.unnamed += 1;
            return;
        }
        let index = listing.outputs.len();
        listing.outputs.push(OutputBuilder::default());
        registry.bind::<WlOutput, _, _>(name, NAME_VERSION, qh, index);
    }
}

impl Dispatch<WlCallback, Round> for Listing {
    fn event(
        listing: &mut Self,
        _: &WlCallback,
        event: wl_callback::Event,
        round: &Round,
        conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if !matches!(event, wl_callback::Event::Done { .. }) {
            return;
        }
        match round {
            Round::Globals => {
                conn.display().sync(qh, Round::Outputs);
            }
            Round::Outputs => listing.done = true,
        }
    }
}

impl Dispatch<WlOutput, usize> for Listing {
    fn event(
        listing: &mut Self,
        _: &WlOutput,
        event: wl_output::Event,
        index: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let Some(output) = listing.outputs.get_mut(*index) {
            output.apply(event);
        }
    }
}

#[cfg(test)]
mod tests;
