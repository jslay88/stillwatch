//! Talking to the session bus: connecting, finding players, and reading
//! their `PlaybackStatus`.

use std::time::Duration;

use stillwatch_core::backend::BackendError;
use zbus::fdo::DBusProxy;
use zbus::message::Type;
use zbus::proxy::CacheProperties;
use zbus::zvariant::OwnedValue;
use zbus::{Connection, MatchRule};

use super::players::{Player, PlayerSet};
use super::properties::{
    OBJECT_PATH, PLAYBACK_STATUS, PLAYER_INTERFACE, ROOT_INTERFACE, is_playing, player_name,
};
use crate::dbus::{self, Bus};

/// A player that doesn't answer within this long counts as not playing, so
/// one hung player can't stall the watcher.
const CALL_TIMEOUT: Duration = Duration::from_secs(5);

const PROPERTIES_INTERFACE: &str = "org.freedesktop.DBus.Properties";

/// The arguments of the one `Get` call made to players.
const STATUS_REQUEST: (&str, &str) = (PLAYER_INTERFACE, PLAYBACK_STATUS);

/// Opens a new connection to `bus`.
pub(crate) async fn connect(bus: &Bus) -> Result<Connection, BackendError> {
    dbus::connect(bus, CALL_TIMEOUT).await
}

/// `NameOwnerChanged` for every `org.mpris.MediaPlayer2.*` name.
pub(crate) fn owner_changes() -> zbus::Result<MatchRule<'static>> {
    Ok(MatchRule::builder()
        .msg_type(Type::Signal)
        .sender("org.freedesktop.DBus")?
        .interface("org.freedesktop.DBus")?
        .member("NameOwnerChanged")?
        .arg0ns(ROOT_INTERFACE)?
        .build())
}

/// `PropertiesChanged` on the player interface, from any player.
pub(crate) fn status_changes() -> zbus::Result<MatchRule<'static>> {
    Ok(MatchRule::builder()
        .msg_type(Type::Signal)
        .interface(PROPERTIES_INTERFACE)?
        .member("PropertiesChanged")?
        .path(OBJECT_PATH)?
        .arg(0, PLAYER_INTERFACE)?
        .build())
}

/// Adds every player currently on the bus to `players`.
pub(crate) async fn discover(
    conn: &Connection,
    players: &mut PlayerSet,
) -> Result<(), BackendError> {
    let dbus = DBusProxy::builder(conn)
        .cache_properties(CacheProperties::No)
        .build()
        .await
        .map_err(disconnected)?;
    let names = dbus.list_names().await.map_err(disconnected)?;
    for name in names {
        let name = name.as_str();
        if player_name(name).is_none() {
            continue;
        }
        // The player can exit between ListNames and here; skip it then.
        let Ok(owner) = dbus
            .get_name_owner(name.try_into().map_err(protocol)?)
            .await
        else {
            continue;
        };
        if let Some(player) = describe(conn, name, owner.as_str()).await {
            players.insert(name.to_owned(), player);
        }
    }
    Ok(())
}

/// Reads a player's status. `None` if `bus_name` isn't an MPRIS name.
pub(crate) async fn describe(conn: &Connection, bus_name: &str, owner: &str) -> Option<Player> {
    let name = player_name(bus_name)?.to_owned();
    Some(Player {
        owner: owner.to_owned(),
        name,
        playing: playing(conn, owner).await,
    })
}

/// Whether the player owned by `owner` reports `Playing`. A player that
/// doesn't answer counts as not playing.
pub(crate) async fn playing(conn: &Connection, owner: &str) -> bool {
    let reply = conn
        .call_method(
            Some(owner),
            OBJECT_PATH,
            Some(PROPERTIES_INTERFACE),
            "Get",
            &STATUS_REQUEST,
        )
        .await;
    match reply {
        Ok(reply) => reply
            .body()
            .deserialize::<OwnedValue>()
            .is_ok_and(|status| is_playing(&status)),
        Err(err) => {
            tracing::debug!(owner, %err, "MPRIS PlaybackStatus read failed");
            false
        }
    }
}

/// Maps a bus-level failure to the transient error the supervisor retries.
pub(crate) fn disconnected(err: impl std::fmt::Display) -> BackendError {
    BackendError::Disconnected(format!("session bus: {err}"))
}

fn protocol(err: impl std::fmt::Display) -> BackendError {
    BackendError::Protocol(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn match_rules_select_mpris_names_and_player_status() {
        assert_eq!(
            owner_changes().unwrap().to_string(),
            "type='signal',sender='org.freedesktop.DBus',interface='org.freedesktop.DBus',\
             member='NameOwnerChanged',arg0namespace='org.mpris.MediaPlayer2'"
        );
        assert_eq!(
            status_changes().unwrap().to_string(),
            "type='signal',interface='org.freedesktop.DBus.Properties',\
             member='PropertiesChanged',path='/org/mpris/MediaPlayer2',\
             arg0='org.mpris.MediaPlayer2.Player'"
        );
    }

    #[test]
    fn playback_status_is_the_only_property_requested() {
        assert_eq!(
            STATUS_REQUEST,
            ("org.mpris.MediaPlayer2.Player", "PlaybackStatus")
        );
    }

    #[test]
    fn bus_errors_are_transient() {
        let err = disconnected(zbus::Error::Failure("gone".into()));
        assert!(err.is_transient(), "{err}");
        assert!(!protocol("bad name").is_transient());
    }
}
