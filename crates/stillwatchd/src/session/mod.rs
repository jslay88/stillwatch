//! Session lock and suspend, from logind on the system bus and
//! `org.freedesktop.ScreenSaver` on the session bus.
//!
//! [`DbusSessionMonitor`] finds the logind session Stillwatch runs in
//! (`XDG_SESSION_ID`, then `GetSessionByPID`, then logind's `auto`) and
//! follows three lock sources: the session's `LockedHint`, its `Lock` /
//! `Unlock` signals, and the screensaver's `ActiveChanged`. One lock shows up
//! on several of them, so they are merged into one `Locked` / `Unlocked` per
//! transition. The manager's `PrepareForSleep(true/false)` becomes
//! `PrepareForSleep` / `ResumedFromSleep`.
//!
//! The screensaver is optional (not every desktop has one); logind is not.

mod logind;
mod screensaver;
mod tracker;
mod watch;

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use stillwatch_core::backend::{BackendError, BackendFuture, EventSink, SessionMonitor};

use self::logind::Lookup;
use self::tracker::Tracker;
use crate::dbus::{self, Bus};

/// Calls that take longer than this fail. Plasma answers `ScreenSaver.Lock`
/// once the lock screen is up, so this leaves it room.
const CALL_TIMEOUT: Duration = Duration::from_secs(10);

/// A [`SessionMonitor`] backed by logind and `org.freedesktop.ScreenSaver`.
///
/// `watch` opens its own connections and returns
/// `Err(BackendError::Disconnected)` when either bus goes away, so the caller
/// can back off and call it again. Run one `watch` at a time.
///
/// The monitor remembers the lock state its consumer last learned, from
/// [`is_locked`](SessionMonitor::is_locked) or a `watch` event. The very
/// first `watch` reports nothing at start; a later one (after a reconnect)
/// reports a lock, unlock, or resume that happened while no watch was
/// running. Calling `is_locked` before the first `watch` closes the gap
/// between the two.
///
/// [`BackendError::Disconnected`]: stillwatch_core::backend::BackendError::Disconnected
#[derive(Debug)]
pub struct DbusSessionMonitor {
    system: Bus,
    session: Bus,
    lookup: Lookup,
    tracker: Mutex<Tracker>,
}

impl DbusSessionMonitor {
    /// Uses the system and session buses, and finds the session through
    /// `XDG_SESSION_ID` or this process's PID.
    #[must_use]
    pub fn new() -> Self {
        Self::on(Bus::System, Bus::Session, Lookup::from_env())
    }

    /// Uses the buses at these addresses (tests use private buses) as the
    /// system and session buses, and finds the session by this process's
    /// PID. `XDG_SESSION_ID` is not read.
    #[must_use]
    pub fn at_addresses(system: impl Into<String>, session: impl Into<String>) -> Self {
        let lookup = Lookup {
            session_id: None,
            pid: std::process::id(),
        };
        Self::on(
            Bus::Address(system.into()),
            Bus::Address(session.into()),
            lookup,
        )
    }

    /// Looks the session up by this ID first, as with `XDG_SESSION_ID`.
    #[must_use]
    pub fn with_session_id(mut self, id: impl Into<String>) -> Self {
        self.lookup.session_id = Some(id.into());
        self
    }

    /// Looks the session up by this PID instead of our own.
    #[must_use]
    pub fn with_pid(mut self, pid: u32) -> Self {
        self.lookup.pid = pid;
        self
    }

    fn on(system: Bus, session: Bus, lookup: Lookup) -> Self {
        Self {
            system,
            session,
            lookup,
            tracker: Mutex::default(),
        }
    }

    fn tracker(&self) -> MutexGuard<'_, Tracker> {
        self.tracker.lock().unwrap_or_else(PoisonError::into_inner)
    }

    async fn locked_now(&self) -> Result<bool, BackendError> {
        let system = dbus::connect(&self.system, CALL_TIMEOUT).await?;
        let session = dbus::connect(&self.session, CALL_TIMEOUT).await?;
        let sources = watch::Sources::open(&system, &session, &self.lookup).await?;
        let snapshot = sources.snapshot().await?;
        let locked = snapshot.locked().unwrap_or(false);
        self.tracker().told(locked);
        Ok(locked)
    }

    async fn lock_now(&self) -> Result<(), BackendError> {
        let Err(refused) = self.screensaver_lock().await else {
            return Ok(());
        };
        tracing::info!(error = %refused, "ScreenSaver.Lock failed, asking logind to lock");
        let system = dbus::connect(&self.system, CALL_TIMEOUT).await?;
        logind::lock_session(&system, &self.lookup).await
    }

    async fn screensaver_lock(&self) -> Result<(), BackendError> {
        let conn = dbus::connect(&self.session, CALL_TIMEOUT).await?;
        screensaver::proxy(&conn)
            .await?
            .lock()
            .await
            .map_err(screensaver::error)
    }
}

impl Default for DbusSessionMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionMonitor for DbusSessionMonitor {
    fn watch(&self, sink: Arc<dyn EventSink>) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            let system = dbus::connect(&self.system, CALL_TIMEOUT).await?;
            let session = dbus::connect(&self.session, CALL_TIMEOUT).await?;
            let sources = watch::Sources::open(&system, &session, &self.lookup).await?;
            watch::run(&sources, &self.tracker, sink.as_ref()).await
        })
    }

    /// Locked if `LockedHint` or the screensaver says so.
    fn is_locked(&self) -> BackendFuture<'_, bool> {
        Box::pin(self.locked_now())
    }

    /// Tries `ScreenSaver.Lock` on the session bus first, then logind's
    /// `LockSession` for our session.
    fn lock(&self) -> BackendFuture<'_, ()> {
        Box::pin(self.lock_now())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_monitor_uses_the_real_buses() {
        let monitor = DbusSessionMonitor::default();
        assert_eq!(monitor.system, Bus::System);
        assert_eq!(monitor.session, Bus::Session);
        assert_eq!(monitor.lookup, Lookup::from_env());
    }

    #[test]
    fn test_monitors_take_addresses_and_lookup_overrides() {
        let monitor = DbusSessionMonitor::at_addresses("unix:path=/a", "unix:path=/b")
            .with_session_id("7")
            .with_pid(42);
        assert_eq!(monitor.system, Bus::Address("unix:path=/a".into()));
        assert_eq!(monitor.session, Bus::Address("unix:path=/b".into()));
        assert_eq!(
            monitor.lookup,
            Lookup {
                session_id: Some("7".into()),
                pid: 42,
            }
        );
    }
}
