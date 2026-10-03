//! One output's overlay: a layer-shell surface on the overlay layer with a
//! single black (or translucent black) buffer.

use smithay_client_toolkit::compositor::{CompositorState, Region};
use smithay_client_toolkit::reexports::client::QueueHandle;
use smithay_client_toolkit::reexports::client::protocol::wl_buffer::WlBuffer;
use smithay_client_toolkit::reexports::client::protocol::wl_output::WlOutput;
use smithay_client_toolkit::shell::WaylandSurface as _;
use smithay_client_toolkit::shell::wlr_layer::{
    Anchor, KeyboardInteractivity, Layer, LayerShell, LayerSurface,
};
use smithay_client_toolkit::shm::Shm;
use smithay_client_toolkit::shm::raw::RawPool;
use stillwatch_core::backend::BackendError;
use wayland_protocols::wp::viewporter::client::wp_viewport::WpViewport;
use wayland_protocols::wp::viewporter::client::wp_viewporter::WpViewporter;

use super::Shade;
use super::state::OverlayState;

/// The layer-shell namespace, so the compositor (and its window rules) can
/// tell these surfaces apart.
pub const NAMESPACE: &str = "stillwatch-overlay";

/// The globals overlays are built from.
pub struct Protocols {
    /// `wl_compositor`.
    pub compositor: CompositorState,
    /// `zwlr_layer_shell_v1`.
    pub layer_shell: LayerShell,
    /// `wl_shm`.
    pub shm: Shm,
    /// `wp_viewporter`, when the compositor has it.
    pub viewporter: Option<WpViewporter>,
}

type Qh = QueueHandle<OverlayState>;

/// A `wl_buffer` that's destroyed with its owner.
struct Buffer(WlBuffer);

impl Drop for Buffer {
    fn drop(&mut self) {
        self.0.destroy();
    }
}

/// One output's overlay surface.
pub struct Overlay {
    /// Connector name of the output it covers.
    pub output: String,
    /// The output it covers.
    pub wl_output: WlOutput,
    shade: Shade,
    // Field order is drop order: the surface goes before its buffer.
    layer: LayerSurface,
    viewport: Option<WpViewport>,
    size: Option<(u32, u32)>,
    buffer: Option<Buffer>,
}

impl Overlay {
    /// Asks for a full-output surface on the overlay layer. It shows nothing
    /// until the compositor's first configure, handled by
    /// [`configure`](Self::configure).
    pub fn create(
        protocols: &Protocols,
        qh: &Qh,
        wl_output: &WlOutput,
        output: &str,
        shade: Shade,
    ) -> Self {
        let surface = protocols.compositor.create_surface(qh);
        let viewport = protocols
            .viewporter
            .as_ref()
            .map(|viewporter| viewporter.get_viewport(&surface, qh, ()));
        let layer = protocols.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Overlay,
            Some(NAMESPACE),
            Some(wl_output),
        );
        layer.set_anchor(Anchor::all());
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_size(0, 0);
        // An empty input region passes pointer input through to whatever is
        // underneath; keyboard input never comes here.
        if let Ok(region) = Region::new(&protocols.compositor) {
            layer
                .wl_surface()
                .set_input_region(Some(region.wl_region()));
        }
        layer.commit();
        Self {
            output: output.to_owned(),
            wl_output: wl_output.clone(),
            shade,
            layer,
            viewport,
            size: None,
            buffer: None,
        }
    }

    /// Whether `layer` is this overlay's surface.
    pub fn is(&self, layer: &LayerSurface) -> bool {
        self.layer.wl_surface() == layer.wl_surface()
    }

    /// The shade it shows, or will show once configured.
    pub const fn shade(&self) -> Shade {
        self.shade
    }

    /// Whether a buffer is attached, so the output is covered.
    pub const fn covering(&self) -> bool {
        self.buffer.is_some()
    }

    /// Takes the compositor's size and shows the buffer. A zero dimension
    /// (the compositor leaving it to us) falls back to `fallback`, the
    /// output's logical size.
    ///
    /// # Errors
    ///
    /// [`BackendError::Io`] when the shared-memory buffer can't be made, or
    /// [`BackendError::Protocol`] when no usable size is known.
    pub fn configure(
        &mut self,
        protocols: &Protocols,
        qh: &Qh,
        size: (u32, u32),
        fallback: Option<(u32, u32)>,
    ) -> Result<(), BackendError> {
        let size = match (size, fallback) {
            ((0, _) | (_, 0), Some(fallback)) => fallback,
            (size, _) => size,
        };
        if size.0 == 0 || size.1 == 0 {
            return Err(BackendError::Protocol(format!(
                "no size for the overlay on {}",
                self.output
            )));
        }
        self.size = Some(size);
        self.render(protocols, qh)
    }

    /// Switches to `shade`, redrawing right away if already configured.
    ///
    /// # Errors
    ///
    /// As [`configure`](Self::configure).
    pub fn set_shade(
        &mut self,
        protocols: &Protocols,
        qh: &Qh,
        shade: Shade,
    ) -> Result<(), BackendError> {
        if shade == self.shade {
            return Ok(());
        }
        self.shade = shade;
        self.render(protocols, qh)
    }

    fn render(&mut self, protocols: &Protocols, qh: &Qh) -> Result<(), BackendError> {
        let Some((width, height)) = self.size else {
            return Ok(());
        };
        let (buffer_width, buffer_height) = match self.viewport {
            Some(_) => (1, 1),
            None => (width, height),
        };
        let buffer = shm_buffer(&protocols.shm, qh, buffer_width, buffer_height, self.shade)?;
        let surface = self.layer.wl_surface();
        surface.attach(Some(&buffer.0), 0, 0);
        if let Some(viewport) = &self.viewport {
            viewport.set_destination(to_i32(width)?, to_i32(height)?);
        }
        surface.damage_buffer(0, 0, to_i32(buffer_width)?, to_i32(buffer_height)?);
        self.layer.commit();
        self.buffer = Some(buffer);
        Ok(())
    }
}

impl Drop for Overlay {
    fn drop(&mut self) {
        if let Some(viewport) = &self.viewport {
            viewport.destroy();
        }
    }
}

/// A `width` x `height` buffer filled with `shade`. The pool is dropped
/// right away; the compositor keeps the memory alive for the buffer.
fn shm_buffer(
    shm: &Shm,
    qh: &Qh,
    width: u32,
    height: u32,
    shade: Shade,
) -> Result<Buffer, BackendError> {
    let too_big =
        || BackendError::Protocol(format!("overlay buffer {width}x{height} is too large"));
    let stride = width.checked_mul(4).ok_or_else(too_big)?;
    let len = usize::try_from(u64::from(stride) * u64::from(height)).map_err(|_| too_big())?;
    let mut pool = RawPool::new(len, shm)
        .map_err(|error| BackendError::Io(format!("can't make the overlay buffer: {error}")))?;
    for pixel in pool.mmap().as_chunks_mut::<4>().0 {
        *pixel = shade.pixel();
    }
    let buffer = pool.create_buffer(
        0,
        to_i32(width)?,
        to_i32(height)?,
        to_i32(stride)?,
        shade.format(),
        (),
        qh,
    );
    Ok(Buffer(buffer))
}

fn to_i32(value: u32) -> Result<i32, BackendError> {
    i32::try_from(value).map_err(|_| BackendError::Protocol(format!("size {value} is too large")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_beyond_i32_are_refused() {
        assert_eq!(to_i32(1920), Ok(1920));
        assert!(matches!(to_i32(u32::MAX), Err(BackendError::Protocol(_))));
    }
}
