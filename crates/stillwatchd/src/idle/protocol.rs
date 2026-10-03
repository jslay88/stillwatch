//! The protocol decisions, kept apart from live Wayland objects so they're
//! unit-testable: which globals to bind at which version, the timeout on the
//! wire, and what each notification event means.

use std::time::Duration;

use stillwatch_core::backend::BackendError;
use stillwatch_core::event::ActivityEvent;
use wayland_client::Proxy as _;
use wayland_client::globals::Global;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_protocols::ext::idle_notify::v1::client::ext_idle_notification_v1;
use wayland_protocols::ext::idle_notify::v1::client::ext_idle_notifier_v1::ExtIdleNotifierV1;

/// `get_input_idle_notification`, which ignores idle inhibitors, arrived in
/// version 2.
pub const MIN_NOTIFIER_VERSION: u32 = 2;

/// The error when the compositor only has the inhibitor-respecting v1.
pub const V1_ONLY: &str =
    "compositor only supports ext-idle-notify v1; input idle that ignores inhibitors is required";

/// The only `wl_seat` request we make is passing it along, which v1 covers.
const SEAT_VERSION: u32 = 1;

/// One global to bind: its registry name and the version to ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bind {
    /// The registry name.
    pub name: u32,
    /// The version to bind at.
    pub version: u32,
}

/// The globals an input idle notification needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    /// The seat whose input is watched.
    pub seat: Bind,
    /// The idle notifier.
    pub notifier: Bind,
}

/// Picks the seat and notifier to bind from the advertised globals.
///
/// The notifier is bound at the highest version both sides know, and never
/// below [`MIN_NOTIFIER_VERSION`].
///
/// # Errors
///
/// [`BackendError::Unsupported`] with [`V1_ONLY`] if the notifier is only v1,
/// and [`BackendError::Unavailable`] if there's no notifier or no seat.
pub fn negotiate(globals: &[Global]) -> Result<Binding, BackendError> {
    let find = |interface: &str| globals.iter().find(|g| g.interface == interface);

    let notifier = find(ExtIdleNotifierV1::interface().name).ok_or_else(|| {
        BackendError::Unavailable("compositor doesn't advertise ext_idle_notifier_v1".into())
    })?;
    if notifier.version < MIN_NOTIFIER_VERSION {
        return Err(BackendError::Unsupported(V1_ONLY.into()));
    }
    let seat = find(WlSeat::interface().name)
        .ok_or_else(|| BackendError::Unavailable("compositor advertises no wl_seat".into()))?;

    Ok(Binding {
        seat: Bind {
            name: seat.name,
            version: seat.version.min(SEAT_VERSION),
        },
        notifier: Bind {
            name: notifier.name,
            version: notifier.version.min(ExtIdleNotifierV1::interface().version),
        },
    })
}

/// The timeout in whole milliseconds, as the protocol wants it. Saturates at
/// `u32::MAX` (about 49 days).
#[must_use]
pub fn timeout_ms(timeout: Duration) -> u32 {
    u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX)
}

/// What a notification event means for the state machine.
#[must_use]
pub fn activity(event: &ext_idle_notification_v1::Event) -> Option<ActivityEvent> {
    match event {
        ext_idle_notification_v1::Event::Idled => Some(ActivityEvent::InputIdle),
        ext_idle_notification_v1::Event::Resumed => Some(ActivityEvent::InputResumed),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn global(name: u32, interface: &str, version: u32) -> Global {
        Global {
            name,
            interface: interface.into(),
            version,
        }
    }

    #[test]
    fn binds_v2_notifier_and_seat() {
        let globals = [
            global(1, "wl_compositor", 6),
            global(11, "wl_seat", 10),
            global(21, "ext_idle_notifier_v1", 2),
        ];
        assert_eq!(
            negotiate(&globals),
            Ok(Binding {
                seat: Bind {
                    name: 11,
                    version: 1
                },
                notifier: Bind {
                    name: 21,
                    version: 2
                },
            })
        );
    }

    #[test]
    fn newer_notifiers_are_bound_at_the_version_we_know() {
        let globals = [
            global(3, "ext_idle_notifier_v1", 7),
            global(4, "wl_seat", 1),
        ];
        let binding = negotiate(&globals).unwrap();
        assert_eq!(binding.notifier.version, 2);
        assert_eq!(binding.seat.version, 1);
    }

    #[test]
    fn v1_only_fails_loudly() {
        let globals = [
            global(1, "ext_idle_notifier_v1", 1),
            global(2, "wl_seat", 9),
        ];
        let err = negotiate(&globals).unwrap_err();
        assert_eq!(err, BackendError::Unsupported(V1_ONLY.into()));
        assert!(!err.is_transient());
        assert!(err.to_string().contains("ignores inhibitors"), "{err}");
    }

    #[test]
    fn missing_notifier_or_seat_is_unavailable() {
        let no_notifier = negotiate(&[global(1, "wl_seat", 9)]).unwrap_err();
        assert!(
            matches!(&no_notifier, BackendError::Unavailable(m) if m.contains("ext_idle_notifier_v1"))
        );
        let no_seat = negotiate(&[global(1, "ext_idle_notifier_v1", 2)]).unwrap_err();
        assert!(matches!(&no_seat, BackendError::Unavailable(m) if m.contains("wl_seat")));
    }

    #[test]
    fn timeout_converts_minutes_to_milliseconds() {
        assert_eq!(timeout_ms(Duration::from_mins(10)), 600_000);
        assert_eq!(timeout_ms(Duration::from_millis(1500)), 1500);
        assert_eq!(timeout_ms(Duration::from_micros(2_999)), 2);
        assert_eq!(timeout_ms(Duration::ZERO), 0);
    }

    #[test]
    fn timeout_saturates_at_u32_max() {
        assert_eq!(timeout_ms(Duration::from_hours(50 * 24)), u32::MAX);
        assert_eq!(timeout_ms(Duration::MAX), u32::MAX);
    }

    #[test]
    fn events_translate_to_activity() {
        assert_eq!(
            activity(&ext_idle_notification_v1::Event::Idled),
            Some(ActivityEvent::InputIdle)
        );
        assert_eq!(
            activity(&ext_idle_notification_v1::Event::Resumed),
            Some(ActivityEvent::InputResumed)
        );
    }
}
