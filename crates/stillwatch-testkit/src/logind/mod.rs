//! A fake systemd-logind: the login manager and one session, for a private
//! bus playing the system bus.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use zbus::Connection;
use zbus::connection::Builder;
use zbus::object_server::{InterfaceRef, SignalEmitter};
use zbus::zvariant::OwnedObjectPath;

use self::objects::{Manager, ManagerSignals as _, Session, SessionSignals as _, State};
use crate::signal::properties_changed;
use crate::sync::lock;
use crate::{Error, PrivateBus};

/// logind's bus name.
pub const SERVICE: &str = "org.freedesktop.login1";

/// The session object's interface.
pub const SESSION_INTERFACE: &str = "org.freedesktop.login1.Session";

/// The manager's object path.
pub const MANAGER_PATH: &str = "/org/freedesktop/login1";

/// The session ID `GetSession("auto")` resolves to here; any other ID but
/// [`Options::session_id`] is unknown.
pub const AUTO: &str = "auto";

/// How the fake is set up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// The one session's ID, as in `XDG_SESSION_ID`.
    pub session_id: String,
    /// Whether `GetSessionByPID` finds the session for any PID. Real logind
    /// finds none for a systemd user service.
    pub pid_in_session: bool,
    /// The session's `LockedHint` at start.
    pub locked_hint: bool,
    /// The manager's `PreparingForSleep` at start.
    pub preparing_for_sleep: bool,
}

impl Default for Options {
    /// Session `3`, found by PID, unlocked and awake.
    fn default() -> Self {
        Self {
            session_id: "3".into(),
            pid_in_session: true,
            locked_hint: false,
            preparing_for_sleep: false,
        }
    }
}

/// logind on its own connection, owning `org.freedesktop.login1`, with one
/// session.
///
/// Every manager method call is recorded (`GetSession 3`,
/// `GetSessionByPID 42`, `LockSession 3`), so tests can check how the
/// session was found and how it was locked.
#[derive(Debug)]
pub struct FakeLogind {
    conn: Connection,
    state: Arc<State>,
    session: FakeSession,
}

impl FakeLogind {
    /// Starts logind with [`Options::default`].
    ///
    /// # Errors
    ///
    /// Fails if the connection or the name request fails.
    pub async fn spawn(bus: &PrivateBus) -> Result<Self, Error> {
        Self::spawn_with(bus, Options::default()).await
    }

    /// Starts logind set up as `options` says.
    ///
    /// # Errors
    ///
    /// Fails if the session ID makes no valid object path, or the connection
    /// or the name request fails.
    pub async fn spawn_with(bus: &PrivateBus, options: Options) -> Result<Self, Error> {
        let path = OwnedObjectPath::try_from(session_path(&options.session_id))
            .map_err(zbus::Error::from)?;
        let state = Arc::new(State {
            locked_hint: AtomicBool::new(options.locked_hint),
            preparing_for_sleep: AtomicBool::new(options.preparing_for_sleep),
            options,
            path: path.clone(),
            calls: Mutex::default(),
            reads: Mutex::default(),
        });
        let conn = Builder::address(bus.address())?
            .serve_at(MANAGER_PATH, Manager(Arc::clone(&state)))?
            .serve_at(&path, Session(Arc::clone(&state)))?
            .name(SERVICE)?
            .build()
            .await?;
        let session = FakeSession {
            conn: conn.clone(),
            state: Arc::clone(&state),
        };
        Ok(Self {
            conn,
            state,
            session,
        })
    }

    /// The one session.
    #[must_use]
    pub const fn session(&self) -> &FakeSession {
        &self.session
    }

    /// Sets `PreparingForSleep` and emits `PrepareForSleep(start)`, as
    /// logind does before suspend (`true`) and after resume (`false`).
    ///
    /// # Errors
    ///
    /// Fails if the signal can't be sent.
    pub async fn prepare_for_sleep(&self, start: bool) -> Result<(), Error> {
        self.state
            .preparing_for_sleep
            .store(start, Ordering::SeqCst);
        let emitter = SignalEmitter::new(&self.conn, MANAGER_PATH)?;
        emitter.prepare_for_sleep(start).await?;
        Ok(())
    }

    /// Every manager method call so far, in order.
    #[must_use]
    pub fn calls(&self) -> Vec<String> {
        lock(&self.state.calls).clone()
    }

    /// Every property clients have read (`LockedHint`, `PreparingForSleep`),
    /// in order.
    #[must_use]
    pub fn reads(&self) -> Vec<String> {
        lock(&self.state.reads).clone()
    }

    /// Drops off the bus as if logind exited.
    ///
    /// # Errors
    ///
    /// Fails if closing the socket fails.
    pub async fn exit(self) -> Result<(), Error> {
        Ok(self.conn.close().await?)
    }
}

/// The fake's session object: its `LockedHint` and `Lock` / `Unlock`
/// signals.
#[derive(Debug)]
pub struct FakeSession {
    conn: Connection,
    state: Arc<State>,
}

impl FakeSession {
    /// The session's object path, escaped the way logind does it (`3` is
    /// `/org/freedesktop/login1/session/_33`).
    #[must_use]
    pub fn path(&self) -> &str {
        self.state.path.as_str()
    }

    /// The current `LockedHint`.
    #[must_use]
    pub fn locked_hint(&self) -> bool {
        self.state.locked_hint.load(Ordering::SeqCst)
    }

    /// Sets `LockedHint` as the lock screen does, and emits
    /// `PropertiesChanged` for it even if the value didn't change, so tests
    /// can send duplicates.
    ///
    /// # Errors
    ///
    /// Fails if the signal can't be sent.
    pub async fn set_locked_hint(&self, locked: bool) -> Result<(), Error> {
        self.state.locked_hint.store(locked, Ordering::SeqCst);
        let session = self.interface().await?;
        session
            .get()
            .await
            .locked_hint_changed(session.signal_emitter())
            .await?;
        Ok(())
    }

    /// Sets `LockedHint` but only lists it as invalidated in
    /// `PropertiesChanged`, so clients have to read it back.
    ///
    /// # Errors
    ///
    /// Fails if the signal can't be sent.
    pub async fn invalidate_locked_hint(&self, locked: bool) -> Result<(), Error> {
        self.state.locked_hint.store(locked, Ordering::SeqCst);
        properties_changed(
            &self.conn,
            self.path(),
            SESSION_INTERFACE,
            HashMap::new(),
            &["LockedHint"],
        )
        .await
    }

    /// Emits the session's `Lock` signal (a request to the lock screen).
    ///
    /// # Errors
    ///
    /// Fails if the signal can't be sent.
    pub async fn emit_lock(&self) -> Result<(), Error> {
        self.emitter()?.lock().await?;
        Ok(())
    }

    /// Emits the session's `Unlock` signal.
    ///
    /// # Errors
    ///
    /// Fails if the signal can't be sent.
    pub async fn emit_unlock(&self) -> Result<(), Error> {
        self.emitter()?.unlock().await?;
        Ok(())
    }

    async fn interface(&self) -> Result<InterfaceRef<Session>, Error> {
        let server = self.conn.object_server();
        Ok(server.interface(&self.state.path).await?)
    }

    fn emitter(&self) -> Result<SignalEmitter<'_>, Error> {
        Ok(SignalEmitter::new(&self.conn, &self.state.path)?)
    }
}

/// logind's session object path for `id`: bytes other than ASCII letters
/// and digits, and a leading digit, become `_` and two hex digits.
fn session_path(id: &str) -> String {
    let mut path = String::from("/org/freedesktop/login1/session/");
    for (index, byte) in id.bytes().enumerate() {
        if byte.is_ascii_alphabetic() || (byte.is_ascii_digit() && index > 0) {
            path.push(char::from(byte));
        } else {
            let _ = write!(path, "_{byte:02x}");
        }
    }
    path
}

mod objects;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_paths_are_escaped_like_logind() {
        assert_eq!(session_path("3"), "/org/freedesktop/login1/session/_33");
        assert_eq!(session_path("c12"), "/org/freedesktop/login1/session/c12");
        assert_eq!(session_path("a-b"), "/org/freedesktop/login1/session/a_2db");
    }

    #[tokio::test]
    async fn an_unknown_session_is_an_error_and_nothing_locks() {
        let Some(bus) = PrivateBus::start().unwrap() else {
            return;
        };
        let logind = FakeLogind::spawn(&bus).await.unwrap();
        let client = bus.connect().await.unwrap();
        let reply = client
            .call_method(
                Some(SERVICE),
                MANAGER_PATH,
                Some("org.freedesktop.login1.Manager"),
                "LockSession",
                &("9",),
            )
            .await;
        let err = reply.unwrap_err().to_string();
        assert!(err.contains("NoSuchSession"), "{err}");
        assert_eq!(logind.calls(), ["LockSession 9"]);
        assert!(!logind.session().locked_hint());
        logind.exit().await.unwrap();
    }
}
