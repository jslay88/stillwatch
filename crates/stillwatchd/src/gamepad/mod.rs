//! Gamepad activity from evdev, which the compositor's idle tracking can't
//! see.
//!
//! Joysticks are the `input` subsystem's `/dev/input/event*` nodes tagged
//! `ID_INPUT_JOYSTICK`. udev's `uaccess` tag lets the logged-in user read
//! them. Every node gets its own async event stream; a udev monitor opens
//! new pads and drops removed ones while running.
//!
//! Ignored pads stay open so the GUI picker can still show their activity.
//! A disabled source (`activity.gamepad = false`) is simply never watched:
//! construction opens nothing.

mod axis;
mod evdev_pad;
mod platform;
mod registry;
mod settings;
mod system;
mod watch;

use std::sync::Arc;
use std::time::Duration;

use stillwatch_core::backend::{BackendFuture, EventSink, GamepadDevice, GamepadSource};
use stillwatch_core::time::{Clock, SystemClock};

pub use settings::GamepadSettings;

use platform::Platform;
use registry::Registry;
use system::UdevPlatform;
use watch::Shared;

/// The minimum gap between two activity events from the same device, so a
/// moving stick doesn't flood the state machine.
pub const ACTIVITY_INTERVAL: Duration = Duration::from_secs(1);

/// The source's logic over any [`Platform`], so tests can script one.
struct Watcher<P> {
    platform: P,
    shared: Arc<Shared>,
}

impl<P: Platform> Watcher<P> {
    fn new(platform: P, settings: &GamepadSettings, clock: Arc<dyn Clock>) -> Self {
        let registry = Registry::new(settings, ACTIVITY_INTERVAL);
        Self {
            platform,
            shared: Arc::new(Shared::new(registry, clock)),
        }
    }

    fn watch(&self, sink: Arc<dyn EventSink>) -> BackendFuture<'_, ()> {
        Box::pin(watch::run(&self.platform, &self.shared, sink))
    }
}

/// Gamepad activity from evdev joysticks, with udev hotplug.
pub struct EvdevGamepadSource(Watcher<UdevPlatform>);

impl EvdevGamepadSource {
    /// A source using the system clock. Opens nothing until
    /// [`watch`](GamepadSource::watch) is called.
    #[must_use]
    pub fn new(settings: &GamepadSettings) -> Self {
        Self::with_clock(settings, Arc::new(SystemClock))
    }

    /// A source that timestamps activity with `clock`.
    #[must_use]
    pub fn with_clock(settings: &GamepadSettings, clock: Arc<dyn Clock>) -> Self {
        Self(Watcher::new(UdevPlatform, settings, clock))
    }

    /// Applies a reloaded deadzone and ignore list. Open devices stay open
    /// and are re-checked against the new ignore list.
    pub fn update_settings(&self, settings: &GamepadSettings) {
        self.0.shared.apply(settings);
    }
}

impl GamepadSource for EvdevGamepadSource {
    fn watch(&self, sink: Arc<dyn EventSink>) -> BackendFuture<'_, ()> {
        self.0.watch(sink)
    }

    fn devices(&self) -> Vec<GamepadDevice> {
        self.0.shared.snapshot()
    }
}

#[cfg(test)]
mod tests;
