use super::BackendFuture;
use crate::luma::{LumaGrid, OutputInfo};

/// Screen capture that produces luma grids, never retained frames.
pub trait ScreenCapture: Send + Sync {
    /// The currently connected outputs.
    fn outputs(&self) -> BackendFuture<'_, Vec<OutputInfo>>;

    /// Captures `output` once and returns its luma, downscaled to
    /// `downscale_width` cells wide with the aspect ratio preserved (never
    /// upscaled).
    ///
    /// The full-resolution frame must be dropped before this returns.
    /// Returns `Err(BackendError::NotFound)` for an unknown output and
    /// `Err(BackendError::PermissionDenied)` when the compositor refuses the
    /// capture. Dropping the future abandons the capture.
    fn capture_luma<'a>(
        &'a self,
        output: &'a str,
        downscale_width: u32,
    ) -> BackendFuture<'a, LumaGrid>;
}
