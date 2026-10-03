use std::sync::Arc;
use std::time::Instant;

use super::{BackendFuture, EventSink};

/// A detected gamepad.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GamepadDevice {
    /// Stable id for this connection (for example the evdev node path).
    pub id: String,
    /// The device's reported name, matched against `gamepad_ignore_devices`.
    pub name: String,
    /// Whether the name matched the ignore list. Ignored devices never emit.
    pub ignored: bool,
    /// When this device last produced input past the deadzone, ignored or not.
    pub last_activity: Option<Instant>,
}

/// Gamepad input, which the compositor's idle tracking doesn't see.
pub trait GamepadSource: Send + Sync {
    /// Watches every gamepad, including ones plugged in later.
    ///
    /// Emits `ActivityEvent::GamepadActivity { device }` for each input from a
    /// non-ignored device that passes the deadzone. Implementations may
    /// coalesce bursts, but must emit at least once per second of continuous
    /// activity. Device errors are handled internally; `Err` means the
    /// source as a whole failed (for example the udev monitor died).
    fn watch(&self, sink: Arc<dyn EventSink>) -> BackendFuture<'_, ()>;

    /// A snapshot of currently connected gamepads, for the GUI picker.
    fn devices(&self) -> Vec<GamepadDevice>;
}
