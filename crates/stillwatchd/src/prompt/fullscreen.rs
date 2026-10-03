//! Whether a fullscreen surface is up.
//!
//! `auto` asks only after notifications are verified not to show over one.
//! [`UnverifiedFullscreen`] does not talk to the compositor.

use stillwatch_core::backend::BackendFuture;

/// Reports whether a fullscreen surface is active.
pub trait FullscreenMonitor: Send + Sync {
    /// `Ok(true)` when a fullscreen surface is active.
    fn is_active(&self) -> BackendFuture<'_, bool>;
}

/// Always reports that nothing is fullscreen, without asking the compositor.
///
/// Installed while [`super::NOTIFICATIONS_HIDDEN_OVER_FULLSCREEN`] is false,
/// so a live Plasma session is never probed from here.
#[derive(Debug, Default, Clone, Copy)]
pub struct UnverifiedFullscreen;

impl FullscreenMonitor for UnverifiedFullscreen {
    fn is_active(&self) -> BackendFuture<'_, bool> {
        Box::pin(async { Ok(false) })
    }
}
