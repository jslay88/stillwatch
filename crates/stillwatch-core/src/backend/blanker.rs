use std::sync::Arc;

use super::{BackendFuture, EventSink};

/// Turns displays off and back on with one blank method.
///
/// Output lists are connector names; an empty list means every connected
/// output.
pub trait Blanker: Send + Sync {
    /// Blanks the outputs. Returns once the request was accepted.
    fn blank<'a>(&'a self, outputs: &'a [String]) -> BackendFuture<'a, ()>;

    /// Wakes the outputs. Unblanking an output that isn't blanked is a no-op.
    fn unblank<'a>(&'a self, outputs: &'a [String]) -> BackendFuture<'a, ()>;

    /// Reports power changes as `Event::DisplayPower { output, on, kind }`, so
    /// the re-blank watchdog can notice a display that woke without input.
    ///
    /// For DPMS this is the compositor's per-output mode; for the overlay,
    /// `on: true` means the overlay surface went away. Blankers that can't
    /// observe power state return `Ok(())` immediately.
    fn watch(&self, sink: Arc<dyn EventSink>) -> BackendFuture<'_, ()>;
}
