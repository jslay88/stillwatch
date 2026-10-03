//! Wait until the compositor drops or a backend's bus name changes owner.
//!
//! This is the reconnect signal. The caller probes again; this does not.

use stillwatch_core::backend::BackendError;
use zbus::Connection;

use super::dbus::{KWIN, LOGIND, NOTIFICATIONS, PORTAL, SCREENSAVER, SCREENSHOT2};
use crate::dbus::{self as bus, Bus};
use crate::peer;

const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Resolves when Wayland drops or a watched name gains, loses, or replaces
/// its owner.
///
/// # Errors
///
/// [`BackendError::Disconnected`] when the Wayland socket can't be opened
/// to begin with. A later drop of that socket is `Ok`: the session changed.
pub async fn until_change() -> Result<(), BackendError> {
    let wayland = crate::wayland::connect_to(None)?;
    let session = bus::connect(&Bus::Session, TIMEOUT).await.ok();
    let system = bus::connect(&Bus::System, TIMEOUT).await.ok();
    tokio::select! {
        result = super::wayland::until_drop(&wayland) => {
            let _ = result;
            Ok(())
        }
        () = session_names(session) => Ok(()),
        () = logind_name(system) => Ok(()),
    }
}

async fn session_names(session: Option<Connection>) {
    let Some(session) = session else {
        std::future::pending::<()>().await;
        return;
    };
    tokio::select! {
        () = replaced(&session, KWIN) => {}
        () = replaced(&session, SCREENSHOT2) => {}
        () = replaced(&session, PORTAL) => {}
        () = replaced(&session, NOTIFICATIONS) => {}
        () = replaced(&session, SCREENSAVER) => {}
    }
}

async fn logind_name(system: Option<Connection>) {
    let Some(system) = system else {
        std::future::pending::<()>().await;
        return;
    };
    replaced(&system, LOGIND).await;
}

async fn replaced(conn: &Connection, name: &str) {
    let _ = peer::until_replaced(conn, name).await;
}
