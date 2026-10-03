//! The D-Bus objects behind [`FakeLogind`](super::FakeLogind). Private, since
//! zbus generates undocumented public signal traits for them.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use zbus::Connection;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::OwnedObjectPath;

use super::{AUTO, Options};
use crate::sync::lock;

#[derive(Debug)]
pub(super) struct State {
    pub options: Options,
    pub path: OwnedObjectPath,
    pub locked_hint: AtomicBool,
    pub preparing_for_sleep: AtomicBool,
    pub calls: Mutex<Vec<String>>,
    pub reads: Mutex<Vec<String>>,
}

impl State {
    fn record(&self, call: String) {
        lock(&self.calls).push(call);
    }

    fn read(&self, property: &str) {
        lock(&self.reads).push(property.to_owned());
    }

    fn session(&self, id: &str) -> Result<OwnedObjectPath, LogindError> {
        if id == self.options.session_id || id == AUTO {
            Ok(self.path.clone())
        } else {
            Err(LogindError::NoSuchSession(format!("no session '{id}'")))
        }
    }
}

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.freedesktop.login1")]
pub(super) enum LogindError {
    #[zbus(error)]
    ZBus(zbus::Error),
    NoSuchSession(String),
    #[zbus(name = "NoSessionForPID")]
    NoSessionForPid(String),
}

pub(super) struct Manager(pub Arc<State>);

#[zbus::interface(name = "org.freedesktop.login1.Manager")]
impl Manager {
    fn get_session(&self, session_id: &str) -> Result<OwnedObjectPath, LogindError> {
        self.0.record(format!("GetSession {session_id}"));
        self.0.session(session_id)
    }

    #[zbus(name = "GetSessionByPID")]
    fn get_session_by_pid(&self, pid: u32) -> Result<OwnedObjectPath, LogindError> {
        self.0.record(format!("GetSessionByPID {pid}"));
        if self.0.options.pid_in_session {
            Ok(self.0.path.clone())
        } else {
            Err(LogindError::NoSessionForPid(format!(
                "PID {pid} does not belong to any known session"
            )))
        }
    }

    /// Like logind, asks the session's lock screen to lock by emitting the
    /// session's `Lock` signal. `LockedHint` doesn't change by itself.
    async fn lock_session(
        &self,
        session_id: &str,
        #[zbus(connection)] conn: &Connection,
    ) -> Result<(), LogindError> {
        self.0.record(format!("LockSession {session_id}"));
        let path = self.0.session(session_id)?;
        Session::lock(&SignalEmitter::new(conn, path)?).await?;
        Ok(())
    }

    #[zbus(property)]
    fn preparing_for_sleep(&self) -> bool {
        self.0.read("PreparingForSleep");
        self.0.preparing_for_sleep.load(Ordering::SeqCst)
    }

    /// Sent before suspend (`true`) and after resume (`false`).
    #[zbus(signal)]
    async fn prepare_for_sleep(emitter: &SignalEmitter<'_>, start: bool) -> zbus::Result<()>;
}

pub(super) struct Session(pub Arc<State>);

#[zbus::interface(name = "org.freedesktop.login1.Session")]
impl Session {
    #[zbus(property)]
    fn id(&self) -> String {
        self.0.options.session_id.clone()
    }

    #[zbus(property)]
    fn locked_hint(&self) -> bool {
        self.0.read("LockedHint");
        self.0.locked_hint.load(Ordering::SeqCst)
    }

    /// Asks the lock screen to lock.
    #[zbus(signal)]
    async fn lock(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    /// Asks the lock screen to unlock.
    #[zbus(signal)]
    async fn unlock(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}
