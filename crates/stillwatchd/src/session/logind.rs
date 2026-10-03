//! logind on the system bus: finding our session, its `LockedHint`, its
//! `Lock` / `Unlock` signals, the manager's `PrepareForSleep`, and
//! `LockSession`.

use stillwatch_core::backend::BackendError;
use zbus::Connection;
use zbus::proxy::CacheProperties;
use zbus::zvariant::OwnedObjectPath;

use crate::dbus::call_error;

pub(crate) const SERVICE: &str = "org.freedesktop.login1";

pub(crate) const SESSION_INTERFACE: &str = "org.freedesktop.login1.Session";

pub(crate) const LOCKED_HINT: &str = "LockedHint";

/// logind resolves this to the caller's session or, for a process outside
/// any session (a systemd user service), the user's display session.
const AUTO_SESSION: &str = "auto";

#[zbus::proxy(
    interface = "org.freedesktop.login1.Manager",
    default_service = "org.freedesktop.login1",
    default_path = "/org/freedesktop/login1"
)]
pub(crate) trait Manager {
    fn get_session(&self, session_id: &str) -> zbus::Result<OwnedObjectPath>;

    #[zbus(name = "GetSessionByPID")]
    fn get_session_by_pid(&self, pid: u32) -> zbus::Result<OwnedObjectPath>;

    fn lock_session(&self, session_id: &str) -> zbus::Result<()>;

    #[zbus(property)]
    fn preparing_for_sleep(&self) -> zbus::Result<bool>;

    #[zbus(signal)]
    fn prepare_for_sleep(&self, start: bool) -> zbus::Result<()>;
}

#[zbus::proxy(
    interface = "org.freedesktop.login1.Session",
    default_service = "org.freedesktop.login1"
)]
pub(crate) trait Session {
    #[zbus(property)]
    fn id(&self) -> zbus::Result<String>;

    #[zbus(property)]
    fn locked_hint(&self) -> zbus::Result<bool>;

    #[zbus(signal)]
    fn lock(&self) -> zbus::Result<()>;

    #[zbus(signal)]
    fn unlock(&self) -> zbus::Result<()>;
}

/// How to find the session Stillwatch runs in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Lookup {
    /// `XDG_SESSION_ID`, tried first. Plasma exports it to the systemd user
    /// manager, so a user service has it too.
    pub session_id: Option<String>,
    /// Tried with `GetSessionByPID` next. Fails for a systemd user service,
    /// which runs outside any session scope.
    pub pid: u32,
}

impl Lookup {
    /// `XDG_SESSION_ID` from the environment and this process's PID.
    pub fn from_env() -> Self {
        Self {
            session_id: std::env::var("XDG_SESSION_ID")
                .ok()
                .filter(|id| !id.is_empty()),
            pid: std::process::id(),
        }
    }
}

/// The manager proxy, uncached so every property read asks logind.
pub(crate) async fn manager(conn: &Connection) -> Result<ManagerProxy<'static>, BackendError> {
    ManagerProxy::builder(conn)
        .cache_properties(CacheProperties::No)
        .build()
        .await
        .map_err(error)
}

/// Finds our session: by `XDG_SESSION_ID`, then by PID, then logind's
/// `auto`.
pub(crate) async fn find_session(
    conn: &Connection,
    manager: &ManagerProxy<'_>,
    lookup: &Lookup,
) -> Result<SessionProxy<'static>, BackendError> {
    let path = find_path(manager, lookup).await?;
    tracing::debug!(session = %path, "found the logind session");
    SessionProxy::builder(conn)
        .path(path)
        .map_err(error)?
        .cache_properties(CacheProperties::No)
        .build()
        .await
        .map_err(error)
}

async fn find_path(
    manager: &ManagerProxy<'_>,
    lookup: &Lookup,
) -> Result<OwnedObjectPath, BackendError> {
    if let Some(id) = &lookup.session_id {
        match manager.get_session(id).await {
            Ok(path) => return Ok(path),
            Err(err) => tracing::debug!(id, %err, "no logind session for XDG_SESSION_ID"),
        }
    }
    match manager.get_session_by_pid(lookup.pid).await {
        Ok(path) => return Ok(path),
        Err(err) => tracing::debug!(pid = lookup.pid, %err, "no logind session for our PID"),
    }
    manager.get_session(AUTO_SESSION).await.map_err(error)
}

/// The session's `LockedHint`.
pub(crate) async fn locked_hint(session: &SessionProxy<'_>) -> Result<bool, BackendError> {
    session.locked_hint().await.map_err(error)
}

/// Asks logind to lock our session, which signals its lock screen.
pub(crate) async fn lock_session(conn: &Connection, lookup: &Lookup) -> Result<(), BackendError> {
    let manager = manager(conn).await?;
    let session = find_session(conn, &manager, lookup).await?;
    let id = session.id().await.map_err(error)?;
    manager.lock_session(&id).await.map_err(error)
}

/// A failed logind call as a [`BackendError`].
pub(crate) fn error(err: zbus::Error) -> BackendError {
    call_error(SERVICE, err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_environment_lookup_uses_our_pid() {
        let lookup = Lookup::from_env();
        assert_eq!(lookup.pid, std::process::id());
        assert_ne!(lookup.session_id.as_deref(), Some(""));
    }
}
