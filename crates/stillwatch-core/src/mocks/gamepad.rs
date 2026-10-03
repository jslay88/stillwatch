use std::sync::{Arc, Mutex};

use super::{ScriptedWatch, WatchEnd};
use crate::backend::{BackendFuture, EventSink, GamepadDevice, GamepadSource};
use crate::event::Event;
use crate::sync::lock;

/// A [`GamepadSource`] with scripted activity and a settable device list.
#[derive(Debug, Default)]
pub struct MockGamepadSource {
    script: ScriptedWatch,
    devices: Mutex<Vec<GamepadDevice>>,
}

impl MockGamepadSource {
    /// A source with no devices and an empty script (`watch` hangs).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues the events and ending for the next `watch` call.
    pub fn push_run(&self, events: Vec<Event>, end: WatchEnd) {
        self.script.push(events, end);
    }

    /// Replaces what `devices` returns.
    pub fn set_devices(&self, devices: Vec<GamepadDevice>) {
        *lock(&self.devices) = devices;
    }

    /// How many times `watch` was called.
    #[must_use]
    pub fn watch_calls(&self) -> usize {
        self.script.calls()
    }
}

impl GamepadSource for MockGamepadSource {
    fn watch(&self, sink: Arc<dyn EventSink>) -> BackendFuture<'_, ()> {
        self.script.watch(sink)
    }

    fn devices(&self) -> Vec<GamepadDevice> {
        lock(&self.devices).clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::ActivityEvent;
    use crate::mocks::{RecordingSink, now_or_never};

    #[test]
    fn emits_scripted_activity_and_lists_devices() {
        let pads = MockGamepadSource::new();
        let activity: Event = ActivityEvent::GamepadActivity {
            device: "/dev/input/event7".into(),
        }
        .into();
        pads.push_run(vec![activity.clone()], WatchEnd::Finish);
        let device = GamepadDevice {
            id: "/dev/input/event7".into(),
            name: "Xbox Wireless Controller".into(),
            ignored: false,
            last_activity: None,
        };
        pads.set_devices(vec![device.clone()]);

        let sink = Arc::new(RecordingSink::new());
        assert_eq!(now_or_never(pads.watch(sink.clone())), Some(Ok(())));
        assert_eq!(sink.events(), vec![activity]);
        assert_eq!(pads.devices(), vec![device]);
        assert_eq!(pads.watch_calls(), 1);
    }
}
