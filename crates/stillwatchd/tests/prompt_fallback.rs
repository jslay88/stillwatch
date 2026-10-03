//! Fallback onto a mock dialog when the notification can't be answered.
//!
//! A private bus only. No compositor, no live session bus, no blanking.

use std::error::Error;
use std::sync::Arc;
use std::time::Duration;

use stillwatch_core::backend::{BackendFuture, Prompter};
use stillwatch_core::config::{PromptStyle, PromptUrgency};
use stillwatch_core::history::{HistoryKind, PromptMedium, PromptReason};
use stillwatch_core::mocks::{CallLog, MemoryHistory, Script};
use stillwatch_core::prompt::{PromptOutcome, PromptRequest, StaleOutput};
use stillwatch_core::time::SystemClock;
use stillwatch_testkit::PrivateBus;
use stillwatch_testkit::notifications::{CLOSED_DISMISSED, FakeNotificationServer};
use stillwatchd::prompt::{
    DialogLauncher, NotificationPrompter, PromptParts, StylePrompter, UnverifiedFullscreen,
};
use tokio::time::{sleep, timeout};

type TestResult = Result<(), Box<dyn Error>>;

const WAIT: Duration = Duration::from_secs(5);
const COUNTDOWN: Duration = Duration::from_secs(45);

fn request() -> PromptRequest {
    PromptRequest {
        countdown: COUNTDOWN,
        presets: vec![Duration::from_mins(15), Duration::from_hours(1)],
        allow_custom: true,
        stale_outputs: vec![StaleOutput {
            output: "HDMI-A-1".into(),
            unchanged_percent: 84,
        }],
    }
}

struct MockDialog {
    requests: CallLog<PromptRequest>,
    outcomes: Script<PromptOutcome>,
}

impl MockDialog {
    fn new() -> Self {
        Self {
            requests: CallLog::new(),
            outcomes: Script::new(),
        }
    }

    fn answer(&self, outcome: PromptOutcome) {
        self.outcomes.push(Ok(outcome));
    }
}

impl DialogLauncher for MockDialog {
    fn launch(&self, request: PromptRequest) -> BackendFuture<'_, PromptOutcome> {
        self.requests.push(request);
        match self.outcomes.pop() {
            Some(result) => Box::pin(std::future::ready(result)),
            None => Box::pin(std::future::pending()),
        }
    }

    fn dismiss(&self) -> BackendFuture<'_, ()> {
        Box::pin(std::future::ready(Ok(())))
    }
}

struct Harness {
    dialog: Arc<MockDialog>,
    history: Arc<MemoryHistory>,
    prompter: Arc<StylePrompter>,
}

impl Harness {
    fn new(address: &str) -> Self {
        let dialog = Arc::new(MockDialog::new());
        let history = Arc::new(MemoryHistory::new());
        let prompter = Arc::new(StylePrompter::new(PromptParts {
            notifications: Arc::new(NotificationPrompter::at_address(
                address,
                PromptUrgency::Critical,
            )),
            dialogs: Arc::clone(&dialog) as Arc<dyn DialogLauncher>,
            fullscreen: Arc::new(UnverifiedFullscreen),
            history: Arc::clone(&history) as Arc<dyn stillwatch_core::backend::HistorySink>,
            clock: Arc::new(SystemClock),
            style: PromptStyle::Notification,
            fallback_to_dialog: true,
            notifications_hidden_over_fullscreen: false,
        }));
        Self {
            dialog,
            history,
            prompter,
        }
    }
}

fn reasons(history: &MemoryHistory) -> Vec<(Option<PromptMedium>, Option<PromptReason>)> {
    history
        .entries()
        .into_iter()
        .map(|entry| {
            assert_eq!(entry.kind, HistoryKind::Prompt);
            (entry.prompt_style, entry.prompt_reason)
        })
        .collect()
}

/// A failure that happens before the user sees anything should hand the
/// dialog almost the whole countdown, not a fresh default.
fn still_the_request_countdown(left: Duration) {
    assert!(left <= COUNTDOWN, "{left:?}");
    assert!(
        COUNTDOWN.saturating_sub(left) < Duration::from_secs(3),
        "dialog countdown {left:?} reset away from {COUNTDOWN:?}"
    );
}

#[tokio::test]
async fn a_missing_server_opens_the_dialog_with_the_countdown() -> TestResult {
    let Some(bus) = PrivateBus::start()? else {
        eprintln!("skipping: dbus-daemon is not installed");
        return Ok(());
    };
    let harness = Harness::new(bus.address());
    harness.dialog.answer(PromptOutcome::Cancel);
    let result = timeout(WAIT, harness.prompter.show(request())).await??;
    assert_eq!(result, PromptOutcome::Cancel);
    let shown = harness.dialog.requests.snapshot();
    assert_eq!(shown.len(), 1);
    still_the_request_countdown(shown[0].countdown);
    assert_eq!(shown[0].presets, request().presets);
    assert_eq!(shown[0].stale_outputs, request().stale_outputs);
    // A missing server and a failing Notify are both Unavailable.
    assert_eq!(
        reasons(&harness.history),
        [
            (
                Some(PromptMedium::Notification),
                Some(PromptReason::Configured)
            ),
            (
                Some(PromptMedium::Dialog),
                Some(PromptReason::FallbackUnavailable)
            ),
        ]
    );
    Ok(())
}

#[tokio::test]
async fn a_failing_notify_opens_the_dialog_with_the_countdown() -> TestResult {
    let Some(bus) = PrivateBus::start()? else {
        eprintln!("skipping: dbus-daemon is not installed");
        return Ok(());
    };
    let server = FakeNotificationServer::spawn(&bus).await?;
    server.fail_notify(true);
    let harness = Harness::new(bus.address());
    harness.dialog.answer(PromptOutcome::Timeout);
    let result = timeout(WAIT, harness.prompter.show(request())).await??;
    assert_eq!(result, PromptOutcome::Timeout);
    let shown = harness.dialog.requests.snapshot();
    assert_eq!(shown.len(), 1);
    still_the_request_countdown(shown[0].countdown);
    assert!(shown[0].allow_custom);
    assert_eq!(
        reasons(&harness.history),
        [
            (
                Some(PromptMedium::Notification),
                Some(PromptReason::Configured)
            ),
            (
                Some(PromptMedium::Dialog),
                Some(PromptReason::FallbackUnavailable)
            ),
        ]
    );
    Ok(())
}

#[tokio::test]
async fn closing_without_an_action_opens_the_dialog_for_the_time_left() -> TestResult {
    let Some(bus) = PrivateBus::start()? else {
        eprintln!("skipping: dbus-daemon is not installed");
        return Ok(());
    };
    let server = FakeNotificationServer::spawn(&bus).await?;
    let harness = Harness::new(bus.address());
    harness.dialog.answer(PromptOutcome::Cancel);
    let prompter = Arc::clone(&harness.prompter);
    let task = tokio::spawn(async move { prompter.show(request()).await });
    let sent = wait_for_notification(&server).await?;
    sleep(Duration::from_secs(1)).await;
    server.close(sent, CLOSED_DISMISSED).await?;
    let result = timeout(WAIT, task).await???;
    assert_eq!(result, PromptOutcome::Cancel);
    let shown = harness.dialog.requests.snapshot();
    assert_eq!(shown.len(), 1);
    let left = shown[0].countdown;
    assert!(left < COUNTDOWN, "countdown was reset to {left:?}");
    assert!(
        left > COUNTDOWN.saturating_sub(Duration::from_secs(4)),
        "dialog countdown {left:?} is not the time left from {COUNTDOWN:?}"
    );
    assert_eq!(shown[0].presets, request().presets);
    assert_eq!(
        reasons(&harness.history),
        [
            (
                Some(PromptMedium::Notification),
                Some(PromptReason::Configured)
            ),
            (
                Some(PromptMedium::Dialog),
                Some(PromptReason::FallbackClosed)
            ),
        ]
    );
    Ok(())
}

async fn wait_for_notification(server: &FakeNotificationServer) -> Result<u32, Box<dyn Error>> {
    let poll = async {
        loop {
            if let Some(note) = server.notifications().into_iter().next() {
                return note.id;
            }
            sleep(Duration::from_millis(5)).await;
        }
    };
    Ok(timeout(WAIT, poll).await?)
}
