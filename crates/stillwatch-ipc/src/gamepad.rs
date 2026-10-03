//! The `Gamepads()` payload.

use std::time::Instant;

use serde::{Deserialize, Serialize};
use stillwatch_core::backend::GamepadDevice;

/// A detected gamepad, for the GUI's ignore-list picker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GamepadInfo {
    /// Stable id for this connection.
    pub id: String,
    /// Reported device name.
    pub name: String,
    /// Whether `gamepad_ignore_devices` matches it.
    pub ignored: bool,
    /// Seconds since its last input past the deadzone, if any was seen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seconds_since_activity: Option<u64>,
}

impl GamepadInfo {
    /// Converts a device snapshot taken by the gamepad source, measuring
    /// activity age from `now`.
    #[must_use]
    pub fn from_device(device: &GamepadDevice, now: Instant) -> Self {
        Self {
            id: device.id.clone(),
            name: device.name.clone(),
            ignored: device.ignored,
            seconds_since_activity: device
                .last_activity
                .map(|at| now.saturating_duration_since(at).as_secs()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::json::{from_json, to_json};

    #[test]
    fn converts_activity_to_age() {
        let seen = Instant::now();
        let device = GamepadDevice {
            id: "/dev/input/event7".into(),
            name: "8BitDo Pro 2".into(),
            ignored: true,
            last_activity: Some(seen),
        };
        let info = GamepadInfo::from_device(&device, seen + Duration::from_secs(3));
        assert_eq!(info.seconds_since_activity, Some(3));
        assert!(info.ignored);

        let idle = GamepadDevice {
            last_activity: None,
            ..device
        };
        assert_eq!(
            GamepadInfo::from_device(&idle, seen).seconds_since_activity,
            None
        );
    }

    #[test]
    fn list_round_trips() {
        let pads = vec![GamepadInfo {
            id: "a".into(),
            name: "Pad".into(),
            ignored: false,
            seconds_since_activity: None,
        }];
        let json = to_json(&pads).unwrap();
        assert_eq!(json, r#"[{"id":"a","name":"Pad","ignored":false}]"#);
        assert_eq!(from_json::<Vec<GamepadInfo>>(&json).unwrap(), pads);
    }
}
