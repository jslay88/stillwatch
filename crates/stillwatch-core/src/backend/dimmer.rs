use super::BackendFuture;

/// Dims displays without turning them off, for the dim step of
/// `action.mode = "dim_then_blank"`.
///
/// Output lists are connector names; an empty list means every connected
/// output.
pub trait Dimmer: Send + Sync {
    /// Dims the outputs so they show `percent` of their normal brightness
    /// (`action.dim_percent`, 0-100; values above 100 count as 100).
    /// Dimming an output again replaces the earlier level. Returns once the
    /// dim is showing.
    fn dim<'a>(&'a self, outputs: &'a [String], percent: u32) -> BackendFuture<'a, ()>;

    /// Removes the dim. Undimming an output that isn't dimmed is a no-op.
    fn undim<'a>(&'a self, outputs: &'a [String]) -> BackendFuture<'a, ()>;
}
