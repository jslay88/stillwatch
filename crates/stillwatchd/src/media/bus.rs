//! Talking to the session bus: connecting, finding players, and reading
//! their `PlaybackStatus`.

use std::time::Duration;

use stillwatch_core::backend::BackendError;
use zbus::fdo::DBusProxy;
use zbus::message::Type;
use zbus::proxy::CacheProperties;
use zbus::zvariant::{OwnedValue, Value};
use zbus::{Connection, MatchRule};

use super::players::{Player, PlayerSet};
use super::properties::{
    IDENTITY, OBJECT_PATH, PLAYBACK_STATUS, PLAYER_INTERFACE, ROOT_INTERFACE, player_name,
};
use crate::dbus::{self, Bus};

/// A player that doesn't answer within this long counts as not playing, so
/// one hung player can't stall the watcher.
const CALL_TIMEOUT: Duration = Duration::from_secs(5);

const PROPERTIES_INTERFACE: &str = "org.freedesktop.DBus.Properties";

/// `Get` arguments for `PlaybackStatus` and the one-time `Identity` read.
const STATUS_REQUEST: (&str, &str) = (PLAYER_INTERFACE, PLAYBACK_STATUS);
const IDENTITY_REQUEST: (&str, &str) = (ROOT_INTERFACE, IDENTITY);

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

/// Reads a player's name, `Identity`, and status. `None` if `bus_name` isn't
/// an MPRIS name. `Identity` is read here and not again for this player.
pub(crate) async fn describe(conn: &Connection, bus_name: &str, owner: &str) -> Option<Player> {
    let name = player_name(bus_name)?.to_owned();
    Some(Player {
        owner: owner.to_owned(),
        name,
        identity: identity(conn, owner).await,
        playing: playing(conn, owner).await,
    })
}

/// `Identity` for the player owned by `owner`. Empty when it doesn't answer.
pub(crate) async fn identity(conn: &Connection, owner: &str) -> String {
    property(conn, owner, IDENTITY_REQUEST.0, IDENTITY_REQUEST.1)
        .await
        .unwrap_or_default()
}

/// Whether the player owned by `owner` reports `Playing`. A player that
/// doesn't answer counts as not playing.
pub(crate) async fn playing(conn: &Connection, owner: &str) -> bool {
    property(conn, owner, STATUS_REQUEST.0, STATUS_REQUEST.1)
        .await
        .is_some_and(|status| status == "Playing")
}

/// One `Get` of a string property. Failures are logged without the value.
async fn property(conn: &Connection, owner: &str, interface: &str, name: &str) -> Option<String> {
    let reply = conn
        .call_method(
            Some(owner),
            OBJECT_PATH,
            Some(PROPERTIES_INTERFACE),
            "Get",
            &(interface, name),
        )
        .await;
    let reply = match reply {
        Ok(reply) => reply,
        Err(err) => {
            tracing::debug!(owner, interface, property = name, %err, "MPRIS property read failed");
            return None;
        }
    };
    let Ok(value) = reply.body().deserialize::<OwnedValue>() else {
        return None;
    };
    match &*value {
        Value::Str(text) => Some(text.as_str().to_owned()),
        _ => None,
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
    fn only_playback_status_and_identity_are_requested() {
        assert_eq!(
            STATUS_REQUEST,
            ("org.mpris.MediaPlayer2.Player", "PlaybackStatus")
        );
        assert_eq!(IDENTITY_REQUEST, ("org.mpris.MediaPlayer2", "Identity"));
    }

    #[test]
    fn bus_errors_are_transient() {
        let err = disconnected(zbus::Error::Failure("gone".into()));
        assert!(err.is_transient(), "{err}");
        assert!(!protocol("bad name").is_transient());
    }
}
