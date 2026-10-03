//! Probe lifetime: the probe runs while at least one `StartProbe` caller is
//! subscribed, and each caller is dropped when it stops or leaves the bus.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::StreamExt as _;
use tokio::task::AbortHandle;
use zbus::Connection;
use zbus::fdo::{DBusProxy, NameOwnerChangedStream};
use zbus::names::BusName;

use super::handle::DaemonHandle;
use super::lock;
use super::signals::ServiceSignals;

/// Aborts its task when dropped, so removing a subscriber or replacing the
/// probe cancels the work it owned.
#[derive(Debug)]
struct AbortOnDrop(AbortHandle);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[derive(Debug)]
struct Running {
    interval: Duration,
    _task: AbortOnDrop,
}

#[derive(Debug, Default)]
struct Subscriptions {
    /// Unique bus name of each subscriber, with the task watching for it to
    /// leave. A peer-to-peer client has no name: it's kept under `""` with no
    /// watch, and only `StopProbe` removes it.
    clients: HashMap<String, Option<AbortOnDrop>>,
    running: Option<Running>,
}

pub(super) struct ProbeHub {
    handle: Arc<dyn DaemonHandle>,
    subscriptions: Mutex<Subscriptions>,
}

impl ProbeHub {
    pub(super) fn new(handle: Arc<dyn DaemonHandle>) -> Arc<Self> {
        Arc::new(Self {
            handle,
            subscriptions: Mutex::default(),
        })
    }

    /// Adds `client` and makes sure the probe runs at `interval`. The latest
    /// interval wins, so a client can change it by calling again.
    pub(super) async fn subscribe(
        self: &Arc<Self>,
        connection: &Connection,
        client: Option<String>,
        interval: Duration,
    ) -> zbus::Result<()> {
        let changes = match &client {
            Some(name) if connection.is_bus() => match owner_changes(connection, name).await? {
                Some(changes) => Some(changes),
                None => return Ok(()),
            },
            _ => None,
        };
        let mut subs = lock(&self.subscriptions);
        if let Entry::Vacant(slot) = subs.clients.entry(client.unwrap_or_default()) {
            let watch = changes.map(|changes| self.watch(slot.key().clone(), changes));
            slot.insert(watch);
        }
        if subs
            .running
            .as_ref()
            .is_none_or(|run| run.interval != interval)
        {
            tracing::debug!(?interval, "probe started");
            subs.running = Some(Running {
                interval,
                _task: self.run(ServiceSignals::new(connection)?, interval),
            });
        }
        Ok(())
    }

    /// Removes `client` (`""` for a peer-to-peer client), stopping the probe
    /// if it was the last one.
    pub(super) fn unsubscribe(&self, client: &str) {
        let mut subs = lock(&self.subscriptions);
        if subs.clients.remove(client).is_some() && subs.clients.is_empty() {
            tracing::debug!("probe stopped");
            subs.running = None;
        }
    }

    fn watch(self: &Arc<Self>, client: String, mut changes: NameOwnerChangedStream) -> AbortOnDrop {
        let hub = Arc::downgrade(self);
        let task = tokio::spawn(async move {
            while let Some(change) = changes.next().await {
                if change.args().is_ok_and(|args| args.new_owner().is_none()) {
                    break;
                }
            }
            if let Some(hub) = hub.upgrade() {
                hub.unsubscribe(&client);
            }
        });
        AbortOnDrop(task.abort_handle())
    }

    fn run(&self, signals: ServiceSignals, interval: Duration) -> AbortOnDrop {
        let mut samples = self.handle.probe(interval);
        let task = tokio::spawn(async move {
            while let Some(sample) = samples.next().await {
                if let Err(err) = signals.probe_sample(&sample).await {
                    tracing::warn!(%err, "can't emit a probe sample");
                }
            }
        });
        AbortOnDrop(task.abort_handle())
    }
}

/// `NameOwnerChanged` for `client`, or `None` if it already left the bus.
///
/// The match rule goes in before the ownership check, so a client that
/// leaves in between still produces a signal on the stream.
async fn owner_changes(
    connection: &Connection,
    client: &str,
) -> zbus::Result<Option<NameOwnerChangedStream>> {
    let dbus = DBusProxy::new(connection).await?;
    let changes = dbus
        .receive_name_owner_changed_with_args(&[(0, client)])
        .await?;
    let name = BusName::try_from(client)?;
    Ok(dbus.name_has_owner(name).await?.then_some(changes))
}

#[cfg(test)]
mod tests;
