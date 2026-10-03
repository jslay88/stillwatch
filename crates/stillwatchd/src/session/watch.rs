//! The watch loop: subscribes to every source, reads where things stand,
//! then reports changes until a bus goes away.

use std::sync::{Mutex, PoisonError};

use futures_util::StreamExt as _;
use stillwatch_core::backend::{BackendError, EventSink};
use zbus::Connection;
use zbus::fdo::{PropertiesChanged, PropertiesProxy};
use zbus::zvariant::Value;

use super::logind::{self, LOCKED_HINT, Lookup, ManagerProxy, SESSION_INTERFACE, SessionProxy};
use super::screensaver::{self, ScreenSaverProxy};
use super::tracker::{Snapshot, Source, Tracker};

/// Proxies for every source, on one system and one session connection.
pub(crate) struct Sources {
    system: Connection,
    session: Connection,
    manager: ManagerProxy<'static>,
    login: SessionProxy<'static>,
    screensaver: ScreenSaverProxy<'static>,
}

impl Sources {
    /// Finds our logind session and sets up the proxies.
    pub async fn open(
        system: &Connection,
        session: &Connection,
        lookup: &Lookup,
    ) -> Result<Self, BackendError> {
        let manager = logind::manager(system).await?;
        let login = logind::find_session(system, &manager, lookup).await?;
        let screensaver = screensaver::proxy(session).await?;
        Ok(Self {
            system: system.clone(),
            session: session.clone(),
            manager,
            login,
            screensaver,
        })
    }

    /// Reads the lock and sleep state. The screensaver is optional.
    pub async fn snapshot(&self) -> Result<Snapshot, BackendError> {
        let locked_hint = logind::locked_hint(&self.login).await?;
        let screensaver = match self.screensaver.get_active().await {
            Ok(active) => Some(active),
            Err(err) => {
                tracing::debug!(%err, "no screensaver lock state");
                None
            }
        };
        let sleeping = match self.manager.preparing_for_sleep().await {
            Ok(sleeping) => sleeping,
            Err(err) => {
                tracing::debug!(%err, "logind PreparingForSleep unreadable");
                false
            }
        };
        Ok(Snapshot {
            locked_hint: Some(locked_hint),
            screensaver,
            sleeping,
        })
    }
}

/// Why [`run`] returned without a bus error.
pub(crate) enum End {
    /// logind or the screensaver name changed owner. Open the proxies again.
    Resubscribe,
}

/// Watches `sources` until either bus connection is lost or a peer restarts.
pub(crate) async fn run(
    sources: &Sources,
    tracker: &Mutex<Tracker>,
    sink: &dyn EventSink,
) -> Result<End, BackendError> {
    // Subscribe before reading, so a change in between arrives as a signal
    // instead of being missed.
    let mut sleeps = sources
        .manager
        .receive_prepare_for_sleep()
        .await
        .map_err(logind::error)?;
    let mut hints = PropertiesProxy::builder(&sources.system)
        .destination(logind::SERVICE)
        .and_then(|builder| builder.path(sources.login.inner().path().to_owned()))
        .map_err(logind::error)?
        .build()
        .await
        .map_err(logind::error)?
        .receive_properties_changed_with_args(&[(0, SESSION_INTERFACE)])
        .await
        .map_err(logind::error)?;
    let mut locks = sources.login.receive_lock().await.map_err(logind::error)?;
    let mut unlocks = sources
        .login
        .receive_unlock()
        .await
        .map_err(logind::error)?;
    let mut actives = sources
        .screensaver
        .receive_active_changed()
        .await
        .map_err(screensaver::error)?;
    let snapshot = sources.snapshot().await?;

    let watching = Watching(tracker);
    for event in watching.update(|tracker| tracker.start(snapshot)) {
        sink.send(event.into());
    }
    let mut login_owner = std::pin::pin!(crate::peer::until_replaced(
        &sources.system,
        logind::SERVICE
    ));
    let mut saver_owner = std::pin::pin!(crate::peer::until_replaced(
        &sources.session,
        screensaver::SERVICE
    ));
    loop {
        let event = tokio::select! {
            signal = sleeps.next() => {
                let start = next(signal)?.args().map(|args| args.start);
                match start {
                    Ok(start) => watching.update(|tracker| tracker.sleep(start)),
                    Err(err) => ignore("PrepareForSleep", &err),
                }
            }
            signal = hints.next() => match locked_hint(&sources.login, &next(signal)?).await? {
                Some(locked) => watching.update(|tracker| tracker.lock(Source::LockedHint, locked)),
                None => None,
            },
            signal = locks.next() => {
                next(signal)?;
                watching.update(|tracker| tracker.lock(Source::Request, true))
            }
            signal = unlocks.next() => {
                next(signal)?;
                watching.update(|tracker| tracker.lock(Source::Request, false))
            }
            signal = actives.next() => match next(signal)?.args() {
                Ok(args) => watching.update(|tracker| tracker.lock(Source::ScreenSaver, args.active)),
                Err(err) => ignore("ActiveChanged", &err),
            },
            () = sources.system.closed() => return Err(closed("system")),
            () = sources.session.closed() => return Err(closed("session")),
            result = &mut login_owner => {
                result?;
                return Ok(End::Resubscribe);
            }
            result = &mut saver_owner => {
                result?;
                return Ok(End::Resubscribe);
            }
        };
        if let Some(event) = event {
            tracing::debug!(?event, "session event");
            sink.send(event.into());
        }
    }
}

/// What a session `PropertiesChanged` says about `LockedHint`, reading it
/// back when it was only invalidated.
async fn locked_hint(
    login: &SessionProxy<'_>,
    signal: &PropertiesChanged,
) -> Result<Option<bool>, BackendError> {
    let args = match signal.args() {
        Ok(args) => args,
        Err(err) => return Ok(ignore("PropertiesChanged", &err)),
    };
    if args.interface_name.as_str() != SESSION_INTERFACE {
        return Ok(None);
    }
    match args.changed_properties.get(LOCKED_HINT) {
        Some(Value::Bool(locked)) => Ok(Some(*locked)),
        None if args.invalidated_properties.contains(&LOCKED_HINT) => {
            logind::locked_hint(login).await.map(Some)
        }
        Some(_) | None => Ok(None),
    }
}

/// The tracker while a watch runs; it learns the watch ended on drop, even
/// when the watch future is cancelled.
struct Watching<'a>(&'a Mutex<Tracker>);

impl Watching<'_> {
    fn update<T>(&self, change: impl FnOnce(&mut Tracker) -> T) -> T {
        change(&mut self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

impl Drop for Watching<'_> {
    fn drop(&mut self) {
        self.update(Tracker::stop);
    }
}

fn next<T>(signal: Option<T>) -> Result<T, BackendError> {
    signal.ok_or_else(|| BackendError::Disconnected("signal stream ended".into()))
}

fn closed(bus: &str) -> BackendError {
    BackendError::Disconnected(format!("{bus} bus connection closed"))
}

fn ignore<T>(signal: &str, err: &zbus::Error) -> Option<T> {
    tracing::debug!(signal, %err, "ignoring a malformed signal");
    None
}
