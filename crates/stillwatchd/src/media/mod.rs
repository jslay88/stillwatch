//! MPRIS media players on the session bus.
//!
//! [`MprisWatcher`] reports which players are `Playing`, so the detector can
//! switch to `media_stale_percent`. It reports every playing player; the
//! detector applies `media_ignore_players`. Players are found with
//! `ListNames` and followed with `NameOwnerChanged`, and their status with
//! `PropertiesChanged`, so nothing polls. `PlaybackStatus` is read when it
//! changes. `Identity` is read once per player from `org.mpris.MediaPlayer2`.
//! `Metadata` is never read.
//!
//! Player names are the bus name without `org.mpris.MediaPlayer2.`
//! (`spotify`, `firefox.instance_1_42`). `Identity` is carried next to that
//! name. `media_ignore_players` matches either one.

mod bus;
mod players;
mod properties;
mod watch;

use std::sync::{Arc, Mutex, PoisonError};

use stillwatch_core::backend::{BackendFuture, EventSink, MediaPlayer, MediaWatcher};

use self::players::PlayerSet;
use crate::dbus::Bus;

/// A [`MediaWatcher`] backed by MPRIS on D-Bus.
///
/// Each `watch` call opens its own connection and returns
/// `Err(BackendError::Disconnected)` when the bus goes away, so the caller can
/// back off and call it again.
///
/// [`BackendError::Disconnected`]: stillwatch_core::backend::BackendError::Disconnected
#[derive(Debug)]
pub struct MprisWatcher {
    bus: Bus,
    live: Live,
}

impl MprisWatcher {
    /// Watches the user's session bus.
    #[must_use]
    pub fn session() -> Self {
        Self::on(Bus::Session)
    }

    /// Watches the bus at `address`, for example a private test bus.
    #[must_use]
    pub fn at_address(address: impl Into<String>) -> Self {
        Self::on(Bus::Address(address.into()))
    }

    fn on(bus: Bus) -> Self {
        Self {
            bus,
            live: Live::default(),
        }
    }
}

impl MediaWatcher for MprisWatcher {
    fn watch(&self, sink: Arc<dyn EventSink>) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            let conn = bus::connect(&self.bus).await?;
            let _clear = ClearOnDrop(&self.live);
            watch::run(&conn, sink.as_ref(), &self.live).await
        })
    }

    /// Served from the running watch when there is one, otherwise read from
    /// the bus.
    fn players(&self) -> BackendFuture<'_, Vec<MediaPlayer>> {
        Box::pin(async move {
            if let Some(players) = self.live.get() {
                return Ok(players);
            }
            let conn = bus::connect(&self.bus).await?;
            let mut players = PlayerSet::default();
            bus::discover(&conn, &mut players).await?;
            Ok(players.all())
        })
    }
}

/// Every player's name as the running watch last saw them; `None` while no
/// watch is running.
#[derive(Debug, Default)]
pub(crate) struct Live(Mutex<Option<Vec<MediaPlayer>>>);

impl Live {
    pub(crate) fn set(&self, players: Option<Vec<MediaPlayer>>) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = players;
    }

    fn get(&self) -> Option<Vec<MediaPlayer>> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// Forgets the live player list when a watch ends or is cancelled.
struct ClearOnDrop<'a>(&'a Live);

impl Drop for ClearOnDrop<'_> {
    fn drop(&mut self) {
        self.0.set(None);
    }
}
