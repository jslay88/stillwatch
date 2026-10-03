use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use stillwatch_core::backend::GamepadDevice;

use super::settings::{GamepadSettings, IgnoreList};

#[derive(Debug)]
struct Entry {
    name: String,
    ignored: bool,
    last_activity: Option<Instant>,
    last_emit: Option<Instant>,
}

/// Open gamepads, the current filter settings, and per-device rate limiting.
#[derive(Debug)]
pub(crate) struct Registry {
    devices: BTreeMap<String, Entry>,
    ignore: IgnoreList,
    deadzone_percent: u8,
    interval: Duration,
}

impl Registry {
    /// `interval` is the minimum gap between two activity events from the
    /// same device.
    pub(crate) fn new(settings: &GamepadSettings, interval: Duration) -> Self {
        Self {
            devices: BTreeMap::new(),
            ignore: IgnoreList::new(&settings.ignore_devices),
            deadzone_percent: settings.deadzone_percent,
            interval,
        }
    }

    /// Swaps in new settings and re-checks every open device's name.
    pub(crate) fn apply(&mut self, settings: &GamepadSettings) {
        self.ignore = IgnoreList::new(&settings.ignore_devices);
        self.deadzone_percent = settings.deadzone_percent;
        for entry in self.devices.values_mut() {
            entry.ignored = self.ignore.matches(&entry.name);
        }
    }

    pub(crate) fn deadzone_percent(&self) -> u8 {
        self.deadzone_percent
    }

    /// Adds (or replaces) a device and returns whether it is ignored.
    pub(crate) fn insert(&mut self, id: String, name: String) -> bool {
        let ignored = self.ignore.matches(&name);
        self.devices.insert(
            id,
            Entry {
                name,
                ignored,
                last_activity: None,
                last_emit: None,
            },
        );
        ignored
    }

    /// Returns whether the device was present.
    pub(crate) fn remove(&mut self, id: &str) -> bool {
        self.devices.remove(id).is_some()
    }

    /// Notes input past the deadzone from `id` and returns whether to emit an
    /// activity event for it: never for ignored or unknown devices, and at
    /// most once per interval otherwise.
    pub(crate) fn record(&mut self, id: &str, now: Instant) -> bool {
        let Some(entry) = self.devices.get_mut(id) else {
            return false;
        };
        entry.last_activity = Some(now);
        if entry.ignored {
            return false;
        }
        let due = entry
            .last_emit
            .is_none_or(|last| now.saturating_duration_since(last) >= self.interval);
        if due {
            entry.last_emit = Some(now);
        }
        due
    }

    /// Every open device, ordered by id.
    pub(crate) fn snapshot(&self) -> Vec<GamepadDevice> {
        self.devices
            .iter()
            .map(|(id, entry)| GamepadDevice {
                id: id.clone(),
                name: entry.name.clone(),
                ignored: entry.ignored,
                last_activity: entry.last_activity,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECOND: Duration = Duration::from_secs(1);

    fn settings(ignore: &[&str]) -> GamepadSettings {
        GamepadSettings {
            deadzone_percent: 20,
            ignore_devices: ignore.iter().map(ToString::to_string).collect(),
        }
    }

    #[test]
    fn insert_applies_the_ignore_list() {
        let mut registry = Registry::new(&settings(&["pedals"]), SECOND);
        assert!(!registry.insert("/dev/input/event3".into(), "Xbox Controller".into()));
        assert!(registry.insert("/dev/input/event4".into(), "Sim Pedals".into()));
        let devices = registry.snapshot();
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].id, "/dev/input/event3");
        assert_eq!(devices[0].name, "Xbox Controller");
        assert!(!devices[0].ignored);
        assert!(devices[1].ignored);
        assert_eq!(registry.deadzone_percent(), 20);
    }

    #[test]
    fn rate_limits_per_device() {
        let start = Instant::now();
        let mut registry = Registry::new(&settings(&[]), SECOND);
        registry.insert("a".into(), "Pad A".into());
        registry.insert("b".into(), "Pad B".into());

        assert!(registry.record("a", start));
        assert!(!registry.record("a", start + Duration::from_millis(500)));
        assert!(registry.record("b", start + Duration::from_millis(500)));
        assert!(!registry.record("a", start + Duration::from_millis(999)));
        assert!(registry.record("a", start + SECOND));
        assert!(!registry.record("a", start + Duration::from_millis(1500)));
        assert!(registry.record("a", start + Duration::from_secs(2)));
    }

    #[test]
    fn activity_is_tracked_even_when_suppressed() {
        let start = Instant::now();
        let mut registry = Registry::new(&settings(&["drifty"]), SECOND);
        registry.insert("pad".into(), "Drifty Pad".into());
        registry.insert("ok".into(), "Good Pad".into());

        assert!(!registry.record("pad", start));
        assert!(registry.record("ok", start));
        assert!(!registry.record("ok", start + Duration::from_millis(10)));

        let devices = registry.snapshot();
        assert_eq!(
            devices[0].last_activity,
            Some(start + Duration::from_millis(10))
        );
        assert_eq!(devices[1].last_activity, Some(start));
    }

    #[test]
    fn unknown_devices_never_emit() {
        let mut registry = Registry::new(&settings(&[]), SECOND);
        assert!(!registry.record("ghost", Instant::now()));
        assert_eq!(registry.snapshot(), []);
    }

    #[test]
    fn apply_rechecks_open_devices_without_dropping_them() {
        let start = Instant::now();
        let mut registry = Registry::new(&settings(&[]), SECOND);
        registry.insert("pad".into(), "Fanatec Wheel".into());
        assert!(registry.record("pad", start));

        registry.apply(&GamepadSettings {
            deadzone_percent: 40,
            ignore_devices: vec!["FANATEC".into()],
        });
        let devices = registry.snapshot();
        assert!(devices[0].ignored);
        assert_eq!(devices[0].last_activity, Some(start));
        assert_eq!(registry.deadzone_percent(), 40);
        assert!(!registry.record("pad", start + Duration::from_secs(5)));

        registry.apply(&settings(&[]));
        assert!(!registry.snapshot()[0].ignored);
        assert!(registry.record("pad", start + Duration::from_secs(6)));
    }

    #[test]
    fn remove_drops_only_that_device() {
        let mut registry = Registry::new(&settings(&[]), SECOND);
        registry.insert("a".into(), "A".into());
        registry.insert("b".into(), "B".into());
        assert!(registry.remove("a"));
        assert!(!registry.remove("a"));
        let devices = registry.snapshot();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].id, "b");
    }

    #[test]
    fn reinserting_resets_the_entry() {
        let start = Instant::now();
        let mut registry = Registry::new(&settings(&[]), SECOND);
        registry.insert("a".into(), "Old".into());
        assert!(registry.record("a", start));
        registry.insert("a".into(), "New".into());
        let devices = registry.snapshot();
        assert_eq!(devices[0].name, "New");
        assert_eq!(devices[0].last_activity, None);
        assert!(registry.record("a", start + Duration::from_millis(1)));
    }
}
