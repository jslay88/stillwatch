//! Daemon calls and the watcher that follows the daemon's bus name.

use std::pin::Pin;
use std::str::FromStr;

use futures_util::StreamExt as _;
use futures_util::stream::Stream;
use stillwatch_core::state::State;
use stillwatch_ipc::BUS_NAME;
use stillwatch_ipc::json::from_json;
use stillwatch_ipc::proxy::StillwatchProxy;
use stillwatch_ipc::status::StatusPayload;
use tokio::sync::mpsc;
use zbus::Connection;
use zbus::fdo::DBusProxy;
use zbus::names::BusName;
use zbus::proxy::CacheProperties;

use crate::error::Error;
use crate::presets;
use crate::shell::{DaemonCall, DaemonEvent, Snapshot};

/// Shown when a tray action runs and the daemon name has no owner.
const NOT_RUNNING: &str = "stillwatchd is not running";

/// Sends `call` through `proxy`.
///
/// # Errors
///
/// Returns the D-Bus error from the method call.
pub async fn dispatch(proxy: &StillwatchProxy<'_>, call: DaemonCall) -> Result<(), Error> {
    let result = match call {
        DaemonCall::Snooze { seconds } => proxy.snooze(seconds).await,
        DaemonCall::CancelSnooze => proxy.cancel_snooze().await,
        DaemonCall::Pause => proxy.pause().await,
        DaemonCall::Resume => proxy.resume().await,
        DaemonCall::Reload => proxy.reload().await.map(|_| ()),
        DaemonCall::StartProbe { interval_ms } => proxy.start_probe(interval_ms).await,
        DaemonCall::StopProbe => proxy.stop_probe().await,
        // `watch` answers this before `dispatch`. The arm keeps the match closed.
        DaemonCall::RefreshDevices => Ok(()),
    };
    result.map_err(Error::from)
}

/// Follows `BUS_NAME` on `connection`: status and signals while the daemon
/// owns the name, [`DaemonEvent::Down`] when it doesn't, and `calls` the
/// whole time.
pub async fn watch(
    connection: Connection,
    mut calls: mpsc::Receiver<DaemonCall>,
    events: mpsc::Sender<DaemonEvent>,
) {
    let dbus = match DBusProxy::new(&connection).await {
        Ok(dbus) => dbus,
        Err(err) => {
            tracing::warn!(%err, "can't read the bus daemon");
            let _ = events.send(DaemonEvent::Down).await;
            return;
        }
    };
    let mut owners = match dbus
        .receive_name_owner_changed_with_args(&[(0, BUS_NAME)])
        .await
    {
        Ok(owners) => owners,
        Err(err) => {
            tracing::warn!(%err, "can't watch the daemon name");
            let _ = events.send(DaemonEvent::Down).await;
            return;
        }
    };
    let mut live = if daemon_owned(&dbus).await == Ok(true) {
        attach(&connection, &events).await
    } else {
        let _ = events.send(DaemonEvent::Down).await;
        None
    };

    loop {
        let wake = tokio::select! {
            biased;
            signal = next_signal(&mut live) => Wake::Signal(signal),
            change = owners.next() => match change {
            None => Wake::OwnerEnded,
            Some(change) => {
                Wake::Appeared(change.args().is_ok_and(|args| args.new_owner().is_some()))
            }
        },
            call = calls.recv() => Wake::Call(call),
        };
        match wake {
            Wake::Call(None) | Wake::OwnerEnded => break,
            Wake::Call(Some(call)) => {
                let proxy = live.as_ref().map(|live| live.proxy.clone());
                on_call(proxy, call, &events).await;
            }
            Wake::Appeared(true) => {
                live = attach(&connection, &events).await;
                if live.is_none() {
                    let _ = events.send(DaemonEvent::Down).await;
                }
            }
            Wake::Appeared(false) | Wake::Signal(None) => {
                live = None;
                let _ = events.send(DaemonEvent::Down).await;
            }
            Wake::Signal(Some(incoming)) => {
                let proxy = live.as_ref().map(|live| live.proxy.clone());
                on_signal(proxy, incoming, &events).await;
            }
        }
    }
}

enum Wake {
    Call(Option<DaemonCall>),
    Appeared(bool),
    OwnerEnded,
    Signal(Option<Incoming>),
}

enum Incoming {
    State(String),
    Config { ok: bool, errors: Vec<String> },
    Probe(String),
    Bad(String),
}

struct Live {
    proxy: StillwatchProxy<'static>,
    incoming: Pin<Box<dyn Stream<Item = Incoming> + Send>>,
}

async fn next_signal(live: &mut Option<Live>) -> Option<Incoming> {
    let Some(live) = live.as_mut() else {
        std::future::pending().await
    };
    live.incoming.next().await
}

async fn daemon_owned(dbus: &DBusProxy<'_>) -> zbus::Result<bool> {
    let name = BusName::try_from(BUS_NAME).map_err(|err| zbus::Error::Failure(err.to_string()))?;
    dbus.name_has_owner(name).await.map_err(zbus::Error::from)
}

async fn attach(connection: &Connection, events: &mpsc::Sender<DaemonEvent>) -> Option<Live> {
    let proxy = match proxy(connection).await {
        Ok(proxy) => proxy,
        Err(err) => {
            tracing::warn!(%err, "can't build the daemon proxy");
            return None;
        }
    };
    let incoming = match incoming(&proxy).await {
        Ok(incoming) => incoming,
        Err(err) => {
            tracing::warn!(%err, "can't subscribe to daemon signals");
            return None;
        }
    };
    let live = Live { proxy, incoming };
    if let Err(err) = publish_status(&live.proxy, events).await {
        tracing::warn!(%err, "initial status");
    }
    Some(live)
}

async fn proxy(connection: &Connection) -> Result<StillwatchProxy<'static>, Error> {
    Ok(StillwatchProxy::builder(connection)
        .cache_properties(CacheProperties::No)
        .build()
        .await?)
}

async fn incoming(
    proxy: &StillwatchProxy<'_>,
) -> Result<Pin<Box<dyn Stream<Item = Incoming> + Send>>, Error> {
    let states = proxy.receive_state_changed().await?;
    let configs = proxy.receive_config_changed().await?;
    let probes = proxy.receive_probe_sample().await?;
    let states = boxed(states.map(|signal| match signal.args() {
        Ok(args) => Incoming::State(args.state().to_string()),
        Err(err) => Incoming::Bad(err.to_string()),
    }));
    let configs = boxed(configs.map(|signal| match signal.args() {
        Ok(args) => Incoming::Config {
            ok: *args.ok(),
            errors: args.errors().clone(),
        },
        Err(err) => Incoming::Bad(err.to_string()),
    }));
    let probes = boxed(probes.map(|signal| match signal.args() {
        Ok(args) => Incoming::Probe((*args.json()).to_owned()),
        Err(err) => Incoming::Bad(err.to_string()),
    }));
    let mut merged = futures_util::stream::SelectAll::new();
    merged.push(states);
    merged.push(configs);
    merged.push(probes);
    Ok(Box::pin(merged))
}

fn boxed(
    stream: impl Stream<Item = Incoming> + Send + 'static,
) -> Pin<Box<dyn Stream<Item = Incoming> + Send>> {
    Box::pin(stream)
}

async fn publish_status(
    proxy: &StillwatchProxy<'_>,
    events: &mpsc::Sender<DaemonEvent>,
) -> Result<(), Error> {
    let json = proxy.status().await?;
    let status: StatusPayload = from_json(&json)?;
    let _ = events
        .send(DaemonEvent::Snapshot(Snapshot::from_status(&status)))
        .await;
    let _ = events
        .send(DaemonEvent::Capture(status.capture_backend))
        .await;
    Ok(())
}

async fn refresh_devices(proxy: &StillwatchProxy<'_>, events: &mpsc::Sender<DaemonEvent>) {
    match crate::settings::load_devices(proxy).await {
        Ok(catalog) => {
            let _ = events.send(DaemonEvent::Devices(catalog)).await;
        }
        Err(err) => tracing::warn!(%err, "device list"),
    }
}

async fn on_call(
    proxy: Option<StillwatchProxy<'static>>,
    call: DaemonCall,
    events: &mpsc::Sender<DaemonEvent>,
) {
    if matches!(call, DaemonCall::RefreshDevices) {
        if let Some(proxy) = proxy {
            refresh_devices(&proxy, events).await;
        }
        return;
    }
    let Some(proxy) = proxy else {
        let _ = events
            .send(DaemonEvent::CallFailed(NOT_RUNNING.to_owned()))
            .await;
        return;
    };
    if let Err(err) = dispatch(&proxy, call).await {
        tracing::warn!(%err, "daemon call failed");
        let _ = events.send(DaemonEvent::CallFailed(err.to_string())).await;
    }
}

async fn on_signal(
    proxy: Option<StillwatchProxy<'static>>,
    incoming: Incoming,
    events: &mpsc::Sender<DaemonEvent>,
) {
    match incoming {
        Incoming::Bad(err) => tracing::warn!(%err, "daemon signal"),
        Incoming::State(name) => on_state(proxy, &name, events).await,
        Incoming::Probe(json) => on_probe(&json, events).await,
        Incoming::Config { ok, errors } => {
            let _ = events.send(DaemonEvent::Config { ok, errors }).await;
            if ok {
                let _ = events.send(DaemonEvent::Presets(presets::load())).await;
            }
        }
    }
}

async fn on_probe(json: &str, events: &mpsc::Sender<DaemonEvent>) {
    // Don't log `json`: a bad sample must not put luma or pixels in the log.
    let Ok(sample) = from_json::<stillwatch_ipc::probe::ProbeSample>(json) else {
        tracing::warn!("ignoring a probe sample that isn't block states");
        return;
    };
    let _ = events
        .send(DaemonEvent::Probe(crate::calibration::view_of(&sample)))
        .await;
}

async fn on_state(
    proxy: Option<StillwatchProxy<'static>>,
    name: &str,
    events: &mpsc::Sender<DaemonEvent>,
) {
    if let Some(proxy) = proxy
        && publish_status(&proxy, events).await.is_ok()
    {
        return;
    }
    match State::from_str(name) {
        Ok(state) => {
            let _ = events.send(DaemonEvent::State(state)).await;
        }
        Err(err) => tracing::warn!(%err, "ignoring state signal"),
    }
}
