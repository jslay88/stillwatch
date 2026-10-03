//! The D-Bus side of the fake server: the interface and what it records.

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex, MutexGuard};

use zbus::fdo;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::OwnedValue;

use super::{CLOSED_BY_CALL, Notification};
use crate::sync::lock;

#[derive(Debug, Default)]
pub(super) struct State {
    pub(super) capabilities: Vec<String>,
    pub(super) fail_notify: bool,
    last_id: u32,
    pub(super) open: BTreeSet<u32>,
    pub(super) notifications: Vec<Notification>,
    pub(super) close_calls: Vec<u32>,
}

/// The arguments of `Notify`, in order.
type NotifyArgs = (
    String,
    u32,
    String,
    String,
    String,
    Vec<String>,
    HashMap<String, OwnedValue>,
    i32,
);

pub(super) struct Server(pub(super) Arc<Mutex<State>>);

impl Server {
    fn state(&self) -> MutexGuard<'_, State> {
        lock(&self.0)
    }
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Server {
    fn get_capabilities(&self) -> Vec<String> {
        self.state().capabilities.clone()
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "Notify's signature is fixed by the notification spec"
    )]
    fn notify(
        &self,
        app_name: String,
        replaces_id: u32,
        app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
    ) -> fdo::Result<u32> {
        let args = (
            app_name,
            replaces_id,
            app_icon,
            summary,
            body,
            actions,
            hints,
            expire_timeout,
        );
        self.state().notify(args)
    }

    async fn close_notification(
        &self,
        id: u32,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        let was_open = {
            let mut state = self.state();
            state.close_calls.push(id);
            state.open.remove(&id)
        };
        if !was_open {
            return Err(fdo::Error::Failed(format!("no notification {id}")));
        }
        Self::notification_closed(&emitter, id, CLOSED_BY_CALL).await?;
        Ok(())
    }

    /// The user picked an action.
    #[zbus(signal)]
    pub(super) async fn action_invoked(
        emitter: &SignalEmitter<'_>,
        id: u32,
        action_key: &str,
    ) -> zbus::Result<()>;

    /// A notification went away, with the spec's reason code.
    #[zbus(signal)]
    pub(super) async fn notification_closed(
        emitter: &SignalEmitter<'_>,
        id: u32,
        reason: u32,
    ) -> zbus::Result<()>;
}

impl State {
    pub(super) fn new(capabilities: &[&str]) -> Self {
        Self {
            capabilities: capabilities.iter().map(|&cap| cap.to_owned()).collect(),
            ..Self::default()
        }
    }

    fn notify(&mut self, args: NotifyArgs) -> fdo::Result<u32> {
        let (app_name, replaces_id, app_icon, summary, body, actions, hints, expire_timeout) = args;
        if self.fail_notify {
            return Err(fdo::Error::Failed("Notify is set to fail".into()));
        }
        let id = if self.open.contains(&replaces_id) {
            replaces_id
        } else {
            self.last_id += 1;
            self.last_id
        };
        self.open.insert(id);
        let mut hint_names: Vec<String> = hints.keys().cloned().collect();
        hint_names.sort();
        self.notifications.push(Notification {
            id,
            app_name,
            replaces_id,
            app_icon,
            summary,
            body,
            actions,
            urgency: hints.get("urgency").and_then(|v| v.downcast_ref().ok()),
            resident: hints.get("resident").and_then(|v| v.downcast_ref().ok()),
            desktop_entry: hints
                .get("desktop-entry")
                .and_then(|v| v.downcast_ref::<&str>().ok())
                .map(str::to_owned),
            hint_names,
            expire_timeout,
        });
        Ok(id)
    }
}
