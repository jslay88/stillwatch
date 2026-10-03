//! A private bus with a fake notification server, and a prompter on it.

use std::error::Error;
use std::sync::Arc;
use std::time::Duration;

use stillwatch_core::backend::{BackendError, Prompter};
use stillwatch_core::config::PromptUrgency;
use stillwatch_core::prompt::{PromptOutcome, PromptRequest, StaleOutput};
use stillwatch_testkit::PrivateBus;
use stillwatch_testkit::notifications::{FakeNotificationServer, Notification};
use stillwatchd::prompt::NotificationPrompter;
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout};

pub type TestResult = Result<(), Box<dyn Error>>;
pub type Shown = JoinHandle<Result<PromptOutcome, BackendError>>;

pub const WAIT: Duration = Duration::from_secs(5);
/// Failures must be reported well within the 5 s call timeout.
pub const FAST: Duration = Duration::from_secs(2);
pub const TICK: Duration = Duration::from_millis(20);
pub const APP_ID: &str = "io.github.jslay88.Stillwatch";

pub fn request() -> PromptRequest {
    PromptRequest {
        countdown: Duration::from_mins(1),
        presets: [15, 60, 180].map(Duration::from_mins).to_vec(),
        allow_custom: true,
        stale_outputs: vec![StaleOutput {
            output: "HDMI-A-1".into(),
            unchanged_percent: 84,
        }],
    }
}

/// A private bus with a fake server on it and a prompter pointed at it.
pub struct Setup {
    pub bus: PrivateBus,
    pub server: FakeNotificationServer,
    pub prompter: Arc<NotificationPrompter>,
}

impl Setup {
    pub async fn start() -> Result<Option<Self>, Box<dyn Error>> {
        Self::with_prompter(|address| {
            NotificationPrompter::at_address(address, PromptUrgency::Critical)
        })
        .await
    }

    pub async fn with_prompter(
        make: impl FnOnce(&str) -> NotificationPrompter,
    ) -> Result<Option<Self>, Box<dyn Error>> {
        let Some(bus) = PrivateBus::start()? else {
            return Ok(None);
        };
        let server = FakeNotificationServer::spawn(&bus).await?;
        let prompter = Arc::new(make(bus.address()));
        Ok(Some(Self {
            bus,
            server,
            prompter,
        }))
    }

    pub fn show(&self, request: PromptRequest) -> Shown {
        let prompter = Arc::clone(&self.prompter);
        tokio::spawn(async move { prompter.show(request).await })
    }

    /// Waits until the server has received `count` `Notify` calls.
    pub async fn notified(&self, count: usize) -> Result<Vec<Notification>, Box<dyn Error>> {
        wait_until(|| {
            let sent = self.server.notifications();
            (sent.len() >= count).then_some(sent)
        })
        .await
    }

    /// Shows the default request and returns the task and its notification.
    pub async fn shown(&self) -> Result<(Shown, Notification), Box<dyn Error>> {
        let task = self.show(request());
        let before = self.server.notifications().len();
        let sent = self.notified(before + 1).await?;
        Ok((task, sent[before].clone()))
    }
}

pub async fn wait_until<T>(mut check: impl FnMut() -> Option<T>) -> Result<T, Box<dyn Error>> {
    let poll = async {
        loop {
            if let Some(value) = check() {
                return value;
            }
            sleep(Duration::from_millis(5)).await;
        }
    };
    Ok(timeout(WAIT, poll).await?)
}

pub async fn outcome(task: Shown) -> Result<Result<PromptOutcome, BackendError>, Box<dyn Error>> {
    Ok(timeout(WAIT, task).await??)
}
