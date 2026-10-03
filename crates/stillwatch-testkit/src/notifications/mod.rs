//! A fake `org.freedesktop.Notifications` server.

use std::sync::{Arc, Mutex};

use zbus::Connection;
use zbus::connection::Builder;
use zbus::object_server::SignalEmitter;

use self::server::{Server, State};
use crate::sync::lock;
use crate::{Error, PrivateBus};

mod server;

/// The well-known name a notification server owns.
pub const BUS_NAME: &str = "org.freedesktop.Notifications";

/// The object path a notification server serves.
pub const OBJECT_PATH: &str = "/org/freedesktop/Notifications";

/// What Plasma 6 reports, which includes `actions` and `persistence`.
pub const PLASMA_CAPABILITIES: [&str; 10] = [
    "body",
    "body-hyperlinks",
    "body-markup",
    "body-images",
    "icon-static",
    "actions",
    "persistence",
    "inline-reply",
    "sound",
    "inhibitions",
];

/// `NotificationClosed` reason: it expired.
pub const CLOSED_EXPIRED: u32 = 1;
/// `NotificationClosed` reason: the user dismissed it.
pub const CLOSED_DISMISSED: u32 = 2;
/// `NotificationClosed` reason: a `CloseNotification` call closed it.
pub const CLOSED_BY_CALL: u32 = 3;

/// One `Notify` call as the server received it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    /// The id the server answered with.
    pub id: u32,
    /// `app_name`.
    pub app_name: String,
    /// `replaces_id`; 0 for a new notification.
    pub replaces_id: u32,
    /// `app_icon`.
    pub app_icon: String,
    /// `summary`.
    pub summary: String,
    /// `body`.
    pub body: String,
    /// `actions`: key and label pairs, flattened.
    pub actions: Vec<String>,
    /// The `urgency` hint, if sent as a byte.
    pub urgency: Option<u8>,
    /// The `resident` hint, if sent as a boolean.
    pub resident: Option<bool>,
    /// The `desktop-entry` hint, if sent as a string.
    pub desktop_entry: Option<String>,
    /// Every hint name that was sent, sorted.
    pub hint_names: Vec<String>,
    /// `expire_timeout`.
    pub expire_timeout: i32,
}

impl Notification {
    /// The action keys, without their labels.
    #[must_use]
    pub fn action_keys(&self) -> Vec<&str> {
        self.actions.iter().step_by(2).map(String::as_str).collect()
    }

    /// The label shown for action `key`.
    #[must_use]
    pub fn action_label(&self, key: &str) -> Option<&str> {
        self.actions
            .chunks(2)
            .find(|pair| pair.first().is_some_and(|k| k == key))
            .and_then(|pair| pair.get(1))
            .map(String::as_str)
    }
}

/// A notification server on its own connection, owning
/// `org.freedesktop.Notifications`.
///
/// It records every `Notify` and `CloseNotification` call, and tests drive
/// the user's side with [`invoke_action`](Self::invoke_action) and
/// [`close`](Self::close). Like a real server, `CloseNotification` on an open
/// notification emits `NotificationClosed` with reason 3, and an unknown
/// `replaces_id` gets a fresh id.
#[derive(Debug)]
pub struct FakeNotificationServer {
    conn: Connection,
    state: Arc<Mutex<State>>,
}

impl FakeNotificationServer {
    /// Starts a server with [`PLASMA_CAPABILITIES`].
    ///
    /// # Errors
    ///
    /// Fails if the connection or the name request fails.
    pub async fn spawn(bus: &PrivateBus) -> Result<Self, Error> {
        Self::with_capabilities(bus, &PLASMA_CAPABILITIES).await
    }

    /// Starts a server that reports `capabilities`.
    ///
    /// # Errors
    ///
    /// Fails if the connection or the name request fails.
    pub async fn with_capabilities(bus: &PrivateBus, capabilities: &[&str]) -> Result<Self, Error> {
        let state = Arc::new(Mutex::new(State::new(capabilities)));
        let conn = Builder::address(bus.address())?
            .serve_at(OBJECT_PATH, Server(Arc::clone(&state)))?
            .name(BUS_NAME)?
            .build()
            .await?;
        Ok(Self { conn, state })
    }

    /// Makes every later `Notify` fail (or succeed again).
    pub fn fail_notify(&self, fail: bool) {
        lock(&self.state).fail_notify = fail;
    }

    /// Every `Notify` call, oldest first.
    #[must_use]
    pub fn notifications(&self) -> Vec<Notification> {
        lock(&self.state).notifications.clone()
    }

    /// Every id passed to `CloseNotification`, oldest first.
    #[must_use]
    pub fn close_calls(&self) -> Vec<u32> {
        lock(&self.state).close_calls.clone()
    }

    /// Ids of the notifications still showing.
    #[must_use]
    pub fn open_ids(&self) -> Vec<u32> {
        lock(&self.state).open.iter().copied().collect()
    }

    /// Clicks action `key` on notification `id`. The notification stays
    /// open, as for a resident notification.
    ///
    /// # Errors
    ///
    /// Fails if the signal can't be sent.
    pub async fn invoke_action(&self, id: u32, key: &str) -> Result<(), Error> {
        Server::action_invoked(&self.emitter()?, id, key).await?;
        Ok(())
    }

    /// Closes notification `id` with `reason` (for example
    /// [`CLOSED_DISMISSED`]) and emits `NotificationClosed`.
    ///
    /// # Errors
    ///
    /// Fails if the signal can't be sent.
    pub async fn close(&self, id: u32, reason: u32) -> Result<(), Error> {
        lock(&self.state).open.remove(&id);
        Server::notification_closed(&self.emitter()?, id, reason).await?;
        Ok(())
    }

    /// Drops off the bus as if the server exited.
    ///
    /// # Errors
    ///
    /// Fails if closing the socket fails.
    pub async fn exit(self) -> Result<(), Error> {
        Ok(self.conn.close().await?)
    }

    fn emitter(&self) -> Result<SignalEmitter<'_>, Error> {
        Ok(SignalEmitter::new(&self.conn, OBJECT_PATH)?)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    async fn notify(client: &Connection, replaces_id: u32) -> zbus::Result<u32> {
        let hints = HashMap::from([
            ("urgency", zbus::zvariant::Value::U8(2)),
            ("resident", zbus::zvariant::Value::Bool(true)),
            ("desktop-entry", zbus::zvariant::Value::from("app")),
        ]);
        let args = (
            "app",
            replaces_id,
            "icon",
            "summary",
            "body",
            vec!["a", "A", "b", "B"],
            hints,
            0_i32,
        );
        client
            .call_method(Some(BUS_NAME), OBJECT_PATH, Some(BUS_NAME), "Notify", &args)
            .await?
            .body()
            .deserialize()
    }

    #[tokio::test]
    async fn records_notify_calls_and_reuses_open_ids() {
        let Some(bus) = PrivateBus::start().unwrap() else {
            return;
        };
        let server = FakeNotificationServer::spawn(&bus).await.unwrap();
        let client = bus.connect().await.unwrap();
        let first = notify(&client, 0).await.unwrap();
        assert_eq!(notify(&client, first).await.unwrap(), first);
        assert_ne!(notify(&client, 999).await.unwrap(), first);

        let sent = server.notifications();
        assert_eq!(sent.len(), 3);
        let note = &sent[1];
        assert_eq!((note.id, note.replaces_id), (first, first));
        assert_eq!(note.urgency, Some(2));
        assert_eq!(note.resident, Some(true));
        assert_eq!(note.desktop_entry.as_deref(), Some("app"));
        assert_eq!(note.hint_names, ["desktop-entry", "resident", "urgency"]);
        assert_eq!(note.action_keys(), ["a", "b"]);
        assert_eq!(note.action_label("b"), Some("B"));
        assert_eq!(note.action_label("c"), None);
        assert_eq!(server.open_ids().len(), 2);

        server.fail_notify(true);
        assert!(notify(&client, 0).await.is_err());
        server.close(first, CLOSED_DISMISSED).await.unwrap();
        assert_eq!(server.open_ids().len(), 1);
        server.exit().await.unwrap();
    }

    #[tokio::test]
    async fn reports_the_configured_capabilities() {
        let Some(bus) = PrivateBus::start().unwrap() else {
            return;
        };
        let _server = FakeNotificationServer::with_capabilities(&bus, &["body"])
            .await
            .unwrap();
        let client = bus.connect().await.unwrap();
        let caps: Vec<String> = client
            .call_method(
                Some(BUS_NAME),
                OBJECT_PATH,
                Some(BUS_NAME),
                "GetCapabilities",
                &(),
            )
            .await
            .unwrap()
            .body()
            .deserialize()
            .unwrap();
        assert_eq!(caps, ["body"]);
    }
}
