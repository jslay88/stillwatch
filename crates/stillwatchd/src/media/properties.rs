//! MPRIS names, the properties Stillwatch reads, and parsing status changes.
//!
//! `PlaybackStatus` is read whenever it changes. `Identity` is read once, from
//! the root interface, when a player appears. Nothing here calls `GetAll`, so
//! `Metadata` (track titles, artists, URLs) is never asked for.

use std::collections::HashMap;

use zbus::zvariant::Value;

/// The object path every MPRIS player serves.
pub(crate) const OBJECT_PATH: &str = "/org/mpris/MediaPlayer2";

/// The root MPRIS interface; player bus names live under it.
pub(crate) const ROOT_INTERFACE: &str = "org.mpris.MediaPlayer2";

/// The player interface, which carries `PlaybackStatus`.
pub(crate) const PLAYER_INTERFACE: &str = "org.mpris.MediaPlayer2.Player";

/// `PlaybackStatus` on the player interface.
pub(crate) const PLAYBACK_STATUS: &str = "PlaybackStatus";

/// `Identity` on the root interface. Read once per player, never `Metadata`.
pub(crate) const IDENTITY: &str = "Identity";

const BUS_NAME_PREFIX: &str = "org.mpris.MediaPlayer2.";

const PLAYING: &str = "Playing";

/// The reported player name for `bus_name`: the part after
/// `org.mpris.MediaPlayer2.`, such as `spotify` or `firefox.instance_1_42`.
/// `None` if it isn't an MPRIS player name.
pub(crate) fn player_name(bus_name: &str) -> Option<&str> {
    bus_name
        .strip_prefix(BUS_NAME_PREFIX)
        .filter(|suffix| !suffix.is_empty())
}

/// What a player-interface `PropertiesChanged` says about `PlaybackStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StatusChange {
    /// The new status is in the signal.
    Playing(bool),
    /// The status changed but the value was left out; `Get` it.
    Invalidated,
    /// The signal is about other properties.
    Unchanged,
}

/// Reads the status out of a `PropertiesChanged` body. Players send
/// `Metadata` in the same signal; it is skipped without being looked at.
pub(crate) fn status_change(
    changed: &HashMap<&str, Value<'_>>,
    invalidated: &[&str],
) -> StatusChange {
    if let Some(status) = changed.get(PLAYBACK_STATUS) {
        StatusChange::Playing(is_playing(status))
    } else if invalidated.contains(&PLAYBACK_STATUS) {
        StatusChange::Invalidated
    } else {
        StatusChange::Unchanged
    }
}

/// Whether a `PlaybackStatus` value means `Playing`.
pub(crate) fn is_playing(status: &Value<'_>) -> bool {
    matches!(status, Value::Str(text) if text.as_str() == PLAYING)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_names_drop_the_mpris_prefix() {
        assert_eq!(player_name("org.mpris.MediaPlayer2.mpv"), Some("mpv"));
        assert_eq!(
            player_name("org.mpris.MediaPlayer2.vlc.instance7389"),
            Some("vlc.instance7389")
        );
        assert_eq!(player_name("org.mpris.MediaPlayer2."), None);
        assert_eq!(player_name("org.mpris.MediaPlayer2"), None);
        assert_eq!(player_name("org.kde.StatusNotifierWatcher"), None);
    }

    #[test]
    fn changed_status_wins_over_invalidation() {
        let changed = HashMap::from([
            ("PlaybackStatus", Value::from("Playing")),
            ("Metadata", Value::from("ignored")),
        ]);
        assert_eq!(
            status_change(&changed, &["PlaybackStatus"]),
            StatusChange::Playing(true)
        );
        let paused = HashMap::from([("PlaybackStatus", Value::from("Paused"))]);
        assert_eq!(status_change(&paused, &[]), StatusChange::Playing(false));
    }

    #[test]
    fn invalidated_and_unrelated_changes() {
        assert_eq!(
            status_change(&HashMap::new(), &["Metadata", "PlaybackStatus"]),
            StatusChange::Invalidated
        );
        let metadata_only = HashMap::from([("Metadata", Value::from("ignored"))]);
        assert_eq!(
            status_change(&metadata_only, &["Volume"]),
            StatusChange::Unchanged
        );
    }

    #[test]
    fn only_the_playing_string_counts_as_playing() {
        assert!(is_playing(&Value::from("Playing")));
        for status in [
            Value::from("Paused"),
            Value::from("Stopped"),
            Value::from("playing"),
            Value::from(1_u32),
        ] {
            assert!(!is_playing(&status), "{status:?}");
        }
    }
}
