//! A fake MPRIS media player.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use zbus::Connection;
use zbus::connection::Builder;
use zbus::fdo::Properties;
use zbus::names::InterfaceName;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::Value;

use crate::sync::lock;
use crate::{Error, PrivateBus};

/// The object path every MPRIS player serves.
pub const OBJECT_PATH: &str = "/org/mpris/MediaPlayer2";

const PLAYER_INTERFACE: &str = "org.mpris.MediaPlayer2.Player";

/// What a fake player reports as its track. Tests can check it never leaks.
pub const SECRET_TITLE: &str = "stillwatch-testkit secret track title";

#[derive(Debug)]
struct State {
    identity: String,
    status: Mutex<String>,
    reads: Mutex<Vec<String>>,
}

impl State {
    fn read(&self, property: &str) {
        lock(&self.reads).push(property.to_owned());
    }
}

struct Root(Arc<State>);

#[zbus::interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    #[zbus(property)]
    fn identity(&self) -> String {
        self.0.read("Identity");
        self.0.identity.clone()
    }
}

struct Player(Arc<State>);

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    #[zbus(property)]
    fn playback_status(&self) -> String {
        self.0.read("PlaybackStatus");
        lock(&self.0.status).clone()
    }

    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, String> {
        self.0.read("Metadata");
        metadata()
    }
}

/// An MPRIS player on its own connection, owning
/// `org.mpris.MediaPlayer2.<suffix>`.
///
/// Every property a client reads through `Get` or `GetAll` is recorded, so
/// tests can assert what a watcher asked for. Status changes are announced the
/// way real players do it: one `PropertiesChanged` that also carries
/// `Metadata`.
#[derive(Debug)]
pub struct FakePlayer {
    conn: Connection,
    state: Arc<State>,
}

impl FakePlayer {
    /// Connects to `bus` and claims the player's name with `status`
    /// (`Playing`, `Paused`, or `Stopped`) already set.
    ///
    /// # Errors
    ///
    /// Fails if the connection or the name request fails.
    pub async fn spawn(
        bus: &PrivateBus,
        suffix: &str,
        identity: &str,
        status: &str,
    ) -> Result<Self, Error> {
        let state = Arc::new(State {
            identity: identity.to_owned(),
            status: Mutex::new(status.to_owned()),
            reads: Mutex::default(),
        });
        let conn = Builder::address(bus.address())?
            .serve_at(OBJECT_PATH, Root(Arc::clone(&state)))?
            .serve_at(OBJECT_PATH, Player(Arc::clone(&state)))?
            .name(format!("org.mpris.MediaPlayer2.{suffix}"))?
            .build()
            .await?;
        Ok(Self { conn, state })
    }

    /// Changes the status and emits `PropertiesChanged` with the new value.
    ///
    /// # Errors
    ///
    /// Fails if the signal can't be sent.
    pub async fn set_status(&self, status: &str) -> Result<(), Error> {
        status.clone_into(&mut lock(&self.state.status));
        let changed = HashMap::from([
            ("PlaybackStatus", Value::from(status)),
            ("Metadata", Value::from(metadata())),
        ]);
        self.properties_changed(changed, &[]).await
    }

    /// Changes the status but only lists it as invalidated, so clients have
    /// to `Get` the new value.
    ///
    /// # Errors
    ///
    /// Fails if the signal can't be sent.
    pub async fn invalidate_status(&self, status: &str) -> Result<(), Error> {
        status.clone_into(&mut lock(&self.state.status));
        self.properties_changed(HashMap::new(), &["PlaybackStatus"])
            .await
    }

    /// Emits a `PropertiesChanged` that only touches `Metadata`, as on a track
    /// change.
    ///
    /// # Errors
    ///
    /// Fails if the signal can't be sent.
    pub async fn change_track(&self) -> Result<(), Error> {
        let changed = HashMap::from([("Metadata", Value::from(metadata()))]);
        self.properties_changed(changed, &[]).await
    }

    /// Every property clients have read, in order.
    #[must_use]
    pub fn reads(&self) -> Vec<String> {
        lock(&self.state.reads).clone()
    }

    /// Drops off the bus as if the player process exited.
    ///
    /// # Errors
    ///
    /// Fails if closing the socket fails.
    pub async fn exit(self) -> Result<(), Error> {
        Ok(self.conn.close().await?)
    }

    async fn properties_changed(
        &self,
        changed: HashMap<&str, Value<'_>>,
        invalidated: &[&str],
    ) -> Result<(), Error> {
        let emitter = SignalEmitter::new(&self.conn, OBJECT_PATH)?;
        let interface =
            InterfaceName::from_static_str(PLAYER_INTERFACE).map_err(zbus::Error::from)?;
        Properties::properties_changed(&emitter, interface, changed, Cow::Borrowed(invalidated))
            .await?;
        Ok(())
    }
}

fn metadata() -> HashMap<String, String> {
    HashMap::from([("xesam:title".to_owned(), SECRET_TITLE.to_owned())])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn get_all_shows_up_as_a_metadata_read() {
        let Some(bus) = PrivateBus::start().unwrap() else {
            return;
        };
        let player = FakePlayer::spawn(&bus, "mpv", "mpv", "Playing")
            .await
            .unwrap();
        let client = bus.connect().await.unwrap();
        client
            .call_method(
                Some("org.mpris.MediaPlayer2.mpv"),
                OBJECT_PATH,
                Some("org.freedesktop.DBus.Properties"),
                "GetAll",
                &(PLAYER_INTERFACE,),
            )
            .await
            .unwrap();
        let reads = player.reads();
        assert!(reads.contains(&"Metadata".to_owned()), "{reads:?}");
        assert!(reads.contains(&"PlaybackStatus".to_owned()), "{reads:?}");
    }
}
