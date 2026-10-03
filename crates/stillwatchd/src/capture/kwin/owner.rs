//! Rebuilds the `ScreenShot2` proxy when `KWin`'s well-known name changes owner.
//!
//! The task does not sleep. [`until_replaced`](crate::peer::until_replaced)
//! waits on `NameOwnerChanged`, and the existing capture call fails on its
//! own if the proxy is stale for one request.

use std::sync::{Arc, Mutex, PoisonError};

use zbus::Connection;
use zbus::proxy::CacheProperties;

use super::proxy::ScreenShot2Proxy;

const SERVICE: &str = "org.kde.KWin.ScreenShot2";

pub(super) fn follow(conn: Connection, slot: Arc<Mutex<ScreenShot2Proxy<'static>>>) {
    tokio::spawn(async move {
        loop {
            if crate::peer::until_replaced(&conn, SERVICE).await.is_err() {
                return;
            }
            match ScreenShot2Proxy::builder(&conn)
                .cache_properties(CacheProperties::No)
                .build()
                .await
            {
                Ok(next) => *slot.lock().unwrap_or_else(PoisonError::into_inner) = next,
                Err(err) => tracing::debug!(%err, "ScreenShot2 isn't back yet"),
            }
        }
    });
}
