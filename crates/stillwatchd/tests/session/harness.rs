//! Two private buses standing in for the system and session buses, with a
//! fake logind and an optional fake screensaver, and a running watch.

use std::error::Error;
use std::sync::Arc;
use std::time::Duration;

use stillwatch_core::backend::{BackendError, EventSink, SessionMonitor};
use stillwatch_core::event::{Event, SessionEvent};
use stillwatch_testkit::PrivateBus;
use stillwatch_testkit::logind::{FakeLogind, Options};
use stillwatch_testkit::screensaver::FakeScreenSaver;
use stillwatchd::session::DbusSessionMonitor;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout};

pub type TestResult = Result<(), Box<dyn Error>>;

const WAIT: Duration = Duration::from_secs(5);
const QUIET: Duration = Duration::from_millis(300);
const POLL: Duration = Duration::from_millis(10);

/// A system bus with logind and a session bus that may have a screensaver.
pub struct Desktop {
    pub system: PrivateBus,
    pub session: PrivateBus,
    pub logind: FakeLogind,
    pub screensaver: Option<FakeScreenSaver>,
}

impl Desktop {
    /// `None` when `dbus-daemon` is missing and not required.
    pub async fn start(
        options: Options,
        screensaver: bool,
    ) -> Result<Option<Self>, Box<dyn Error>> {
        let (Some(system), Some(session)) = (PrivateBus::start()?, PrivateBus::start()?) else {
            return Ok(None);
        };
        let logind = FakeLogind::spawn_with(&system, options).await?;
        let screensaver = if screensaver {
            Some(FakeScreenSaver::spawn(&session).await?)
        } else {
            None
        };
        Ok(Some(Self {
            system,
            session,
            logind,
            screensaver,
        }))
    }

    pub fn monitor(&self) -> Arc<DbusSessionMonitor> {
        Arc::new(self.unshared_monitor())
    }

    pub fn unshared_monitor(&self) -> DbusSessionMonitor {
        DbusSessionMonitor::at_addresses(self.system.address(), self.session.address())
    }

    pub fn screensaver(&self) -> Result<&FakeScreenSaver, Box<dyn Error>> {
        Ok(self.screensaver.as_ref().ok_or("no screensaver")?)
    }

    fn snapshots(&self) -> usize {
        self.logind
            .reads()
            .iter()
            .filter(|read| *read == "PreparingForSleep")
            .count()
    }
}

/// A running `watch` with its events collected.
pub struct Watch {
    events: mpsc::UnboundedReceiver<Event>,
    task: JoinHandle<Result<(), BackendError>>,
}

impl Watch {
    /// Starts a watch and waits until it has subscribed and read the state
    /// (its last read is `PreparingForSleep`), so no signal sent after this
    /// returns can be missed.
    pub async fn start(
        desktop: &Desktop,
        monitor: &Arc<DbusSessionMonitor>,
    ) -> Result<Self, Box<dyn Error>> {
        let before = desktop.snapshots();
        let (tx, events) = mpsc::unbounded_channel();
        let sink: Arc<dyn EventSink> = Arc::new(move |event| {
            let _ = tx.send(event);
        });
        let task = tokio::spawn({
            let monitor = Arc::clone(monitor);
            async move { monitor.watch(sink).await }
        });
        timeout(WAIT, async {
            while desktop.snapshots() == before {
                sleep(POLL).await;
            }
        })
        .await?;
        Ok(Self { events, task })
    }

    pub async fn next(&mut self) -> Result<SessionEvent, Box<dyn Error>> {
        let event = timeout(WAIT, self.events.recv())
            .await?
            .ok_or("the watch stopped")?;
        match event {
            Event::Session(event) => Ok(event),
            other => Err(format!("unexpected event {other:?}").into()),
        }
    }

    pub async fn assert_quiet(&mut self) -> TestResult {
        match timeout(QUIET, self.events.recv()).await {
            Err(_) => Ok(()),
            Ok(event) => Err(format!("expected no event, got {event:?}").into()),
        }
    }

    pub async fn ended(mut self) -> Result<BackendError, Box<dyn Error>> {
        let result = timeout(WAIT, &mut self.task).await??;
        Ok(result.err().ok_or("watch ended cleanly")?)
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.task.abort();
    }
}
