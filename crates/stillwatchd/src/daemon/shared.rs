//! State the D-Bus handle reads without waiting for the event loop.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use stillwatch_core::backend::{
    GamepadSource, HistorySink, MediaPlayer, MediaWatcher, ScreenCapture,
};
use stillwatch_core::config::{Config, PromptConfig};
use tokio::sync::mpsc;

use super::inbox::Incoming;
use super::parts::Built;

/// Shared with [`super::handle::Handle`]. The loop writes it; the handle
/// reads it. Nothing here is waited on while a lock is held.
pub(super) struct Shared {
    /// Prompt rules for `validate_snooze`, updated on each applied reload.
    pub prompt: Mutex<PromptConfig>,
    /// The config in effect, for the probe's own detector.
    pub config: Mutex<Config>,
    /// Errors from the last rejected reload. Empty when the config is good.
    pub errors: Mutex<Vec<String>>,
    /// `kwin` while `ScreenShot2` is usable, otherwise input-idle-only.
    pub capture_backend: Mutex<Option<String>>,
    /// Selected backends and why.
    pub backends: Mutex<Option<stillwatch_ipc::status::BackendReport>>,
    /// The capture backend, if one connected.
    pub capture: Mutex<Option<Arc<dyn ScreenCapture>>>,
    /// Portal session for the same backend, when `capture.backend` is portal.
    pub portal: Mutex<Option<Arc<dyn super::parts::CaptureSession>>>,
    /// Gamepads for the picker. `None` when gamepad input is off.
    pub gamepad: Mutex<Option<Arc<dyn GamepadSource>>>,
    /// Players the media watcher last reported as `Playing`.
    pub playing: Mutex<Vec<MediaPlayer>>,
    /// MPRIS names for `Players()`.
    pub media: Arc<dyn MediaWatcher>,
    /// Decision history.
    pub history: Arc<dyn HistorySink>,
    /// Into the event loop.
    pub out: mpsc::UnboundedSender<Incoming>,
}

impl Shared {
    pub(super) fn new(built: &Built, out: mpsc::UnboundedSender<Incoming>) -> Self {
        Self {
            prompt: Mutex::new(built.config.prompt.clone()),
            config: Mutex::new(built.config.clone()),
            errors: Mutex::new(Vec::new()),
            capture_backend: Mutex::new(built.capture_backend.clone()),
            backends: Mutex::new(built.backends.clone()),
            capture: Mutex::new(built.capture.clone()),
            portal: Mutex::new(built.portal.clone()),
            gamepad: Mutex::new(built.gamepad.clone()),
            playing: Mutex::new(Vec::new()),
            media: Arc::clone(&built.media),
            history: Arc::clone(&built.history),
            out,
        }
    }
}

pub(super) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
