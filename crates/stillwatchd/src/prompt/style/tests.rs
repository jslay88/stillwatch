use std::sync::Arc;
use std::time::Duration;

use stillwatch_core::backend::{BackendError, BackendFuture, Prompter};
use stillwatch_core::config::{PromptConfig, PromptStyle, PromptUrgency};
use stillwatch_core::history::{HistoryKind, PromptMedium, PromptReason};
use stillwatch_core::mocks::{CallLog, MemoryHistory, MockPrompter, Script, now_or_never};
use stillwatch_core::prompt::{PromptOutcome, PromptRequest, Reminder, StaleOutput};
use stillwatch_core::time::FakeClock;
use tokio::sync::oneshot;

use super::StylePrompter;
use crate::prompt::PromptParts;
use crate::prompt::dialog::DialogLauncher;
use crate::prompt::fullscreen::FullscreenMonitor;

fn request(seconds: u64) -> PromptRequest {
    PromptRequest {
        countdown: Duration::from_secs(seconds),
        presets: vec![Duration::from_mins(15)],
        allow_custom: false,
        stale_outputs: vec![StaleOutput {
            output: "HDMI-A-1".into(),
            unchanged_percent: 84,
        }],
    }
}

struct Parts {
    notifications: Arc<MockPrompter>,
    dialogs: Arc<MockDialog>,
    fullscreen: Arc<MockFullscreen>,
    history: Arc<MemoryHistory>,
    clock: FakeClock,
    prompter: StylePrompter,
}

struct Built {
    parts: Parts,
}

impl Built {
    fn new(style: PromptStyle, fallback: bool, hidden: bool) -> Self {
        let notifications = Arc::new(MockPrompter::new());
        let dialogs = Arc::new(MockDialog::new());
        let fullscreen = Arc::new(MockFullscreen::new(Ok(false)));
        let history = Arc::new(MemoryHistory::new());
        let clock = FakeClock::new();
        let prompter = StylePrompter::new(PromptParts {
            notifications: Arc::clone(&notifications) as Arc<dyn Prompter>,
            dialogs: Arc::clone(&dialogs) as Arc<dyn DialogLauncher>,
            fullscreen: Arc::clone(&fullscreen) as Arc<dyn FullscreenMonitor>,
            history: Arc::clone(&history) as _,
            clock: Arc::new(clock.clone()),
            style,
            fallback_to_dialog: fallback,
            notifications_hidden_over_fullscreen: hidden,
        });
        Self {
            parts: Parts {
                notifications,
                dialogs,
                fullscreen,
                history,
                clock,
                prompter,
            },
        }
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

    fn push(&self, outcome: PromptOutcome) {
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

struct MockFullscreen {
    result: std::sync::Mutex<Result<bool, BackendError>>,
    calls: CallLog<()>,
}

impl MockFullscreen {
    fn new(result: Result<bool, BackendError>) -> Self {
        Self {
            result: std::sync::Mutex::new(result),
            calls: CallLog::new(),
        }
    }
}

impl FullscreenMonitor for MockFullscreen {
    fn is_active(&self) -> BackendFuture<'_, bool> {
        self.calls.push(());
        let result = self
            .result
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        Box::pin(std::future::ready(result))
    }
}

fn shown(parts: &Parts) -> Vec<(PromptMedium, PromptReason)> {
    parts
        .history
        .entries()
        .into_iter()
        .map(|entry| {
            assert_eq!(entry.kind, HistoryKind::Prompt);
            (entry.prompt_style.unwrap(), entry.prompt_reason.unwrap())
        })
        .collect()
}

#[test]
fn notification_style_never_opens_the_dialog() {
    let built = Built::new(PromptStyle::Notification, true, true);
    let snooze = PromptOutcome::Snooze(Duration::from_mins(15));
    built.parts.notifications.push_outcome(snooze);
    let result = now_or_never(built.parts.prompter.show(request(60)));
    assert_eq!(result, Some(Ok(snooze)));
    assert_eq!(built.parts.dialogs.requests.snapshot(), []);
    assert_eq!(built.parts.fullscreen.calls.snapshot(), []);
    assert_eq!(
        shown(&built.parts),
        [(PromptMedium::Notification, PromptReason::Configured)]
    );
}

#[test]
fn dialog_style_skips_the_notification() {
    let built = Built::new(PromptStyle::Dialog, true, true);
    built.parts.dialogs.push(PromptOutcome::Cancel);
    let result = now_or_never(built.parts.prompter.show(request(45)));
    assert_eq!(result, Some(Ok(PromptOutcome::Cancel)));
    assert_eq!(built.parts.notifications.requests(), []);
    assert_eq!(built.parts.fullscreen.calls.snapshot(), []);
    let shown_request = built.parts.dialogs.requests.snapshot();
    assert_eq!(shown_request[0].countdown, Duration::from_secs(45));
    assert_eq!(
        shown(&built.parts),
        [(PromptMedium::Dialog, PromptReason::Configured)]
    );
}

#[test]
fn auto_ignores_fullscreen_until_hiding_is_verified() {
    let built = Built::new(PromptStyle::Auto, true, false);
    *built
        .parts
        .fullscreen
        .result
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Ok(true);
    built
        .parts
        .notifications
        .push_outcome(PromptOutcome::Timeout);
    let result = now_or_never(built.parts.prompter.show(request(60)));
    assert_eq!(result, Some(Ok(PromptOutcome::Timeout)));
    assert_eq!(built.parts.fullscreen.calls.snapshot(), []);
    assert_eq!(built.parts.dialogs.requests.snapshot(), []);
    assert_eq!(
        shown(&built.parts),
        [(PromptMedium::Notification, PromptReason::Auto)]
    );
}

#[test]
fn auto_uses_the_dialog_when_fullscreen_hides_notifications() {
    let built = Built::new(PromptStyle::Auto, true, true);
    *built
        .parts
        .fullscreen
        .result
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Ok(true);
    built.parts.dialogs.push(PromptOutcome::Cancel);
    let result = now_or_never(built.parts.prompter.show(request(60)));
    assert_eq!(result, Some(Ok(PromptOutcome::Cancel)));
    assert_eq!(built.parts.fullscreen.calls.len(), 1);
    assert_eq!(built.parts.notifications.requests(), []);
    assert_eq!(
        shown(&built.parts),
        [(PromptMedium::Dialog, PromptReason::Fullscreen)]
    );
}

#[test]
fn a_failed_fullscreen_check_stays_on_the_notification() {
    let built = Built::new(PromptStyle::Auto, true, true);
    *built
        .parts
        .fullscreen
        .result
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) =
        Err(BackendError::Unavailable("kwin".into()));
    built
        .parts
        .notifications
        .push_outcome(PromptOutcome::Cancel);
    let result = now_or_never(built.parts.prompter.show(request(60)));
    assert_eq!(result, Some(Ok(PromptOutcome::Cancel)));
    assert_eq!(
        shown(&built.parts),
        [(PromptMedium::Notification, PromptReason::Auto)]
    );
}

#[test]
fn fallback_keeps_the_remaining_countdown() {
    let cases = [
        (
            Err(BackendError::Unavailable("no server".into())),
            PromptReason::FallbackUnavailable,
        ),
        (
            Err(BackendError::Unsupported("no actions".into())),
            PromptReason::FallbackFailed,
        ),
        (Ok(PromptOutcome::Dismissed), PromptReason::FallbackClosed),
    ];
    for (notification, reason) in cases {
        let built = Built::new(PromptStyle::Auto, true, false);
        match notification {
            Ok(outcome) => built.parts.notifications.push_outcome(outcome),
            Err(error) => built.parts.notifications.push_error(error),
        }
        built.parts.dialogs.push(PromptOutcome::Cancel);
        let clock = built.parts.clock.clone();
        let notifications = Arc::clone(&built.parts.notifications);
        let wrapped = Advancing {
            clock: clock.clone(),
            by: Duration::from_secs(12),
            inner: notifications,
        };
        let prompter = StylePrompter::new(PromptParts {
            notifications: Arc::new(wrapped),
            dialogs: Arc::clone(&built.parts.dialogs) as _,
            fullscreen: Arc::clone(&built.parts.fullscreen) as _,
            history: Arc::clone(&built.parts.history) as _,
            clock: Arc::new(clock),
            style: PromptStyle::Auto,
            fallback_to_dialog: true,
            notifications_hidden_over_fullscreen: false,
        });
        let result = now_or_never(prompter.show(request(45)));
        assert_eq!(result, Some(Ok(PromptOutcome::Cancel)));
        let dialogs = built.parts.dialogs.requests.snapshot();
        assert_eq!(dialogs.len(), 1, "{reason:?}");
        assert_eq!(dialogs[0].countdown, Duration::from_secs(33), "{reason:?}");
        assert_eq!(dialogs[0].stale_outputs, request(45).stale_outputs);
        assert_eq!(
            shown(&built.parts),
            [
                (PromptMedium::Notification, PromptReason::Auto),
                (PromptMedium::Dialog, reason),
            ]
        );
    }
}

#[test]
fn fallback_off_returns_the_notification_result() {
    let built = Built::new(PromptStyle::Notification, false, false);
    built
        .parts
        .notifications
        .push_error(BackendError::Unavailable("no server".into()));
    let failed = now_or_never(built.parts.prompter.show(request(45)));
    assert!(matches!(failed, Some(Err(BackendError::Unavailable(_)))));
    built
        .parts
        .notifications
        .push_outcome(PromptOutcome::Dismissed);
    let dismissed = now_or_never(built.parts.prompter.show(request(45)));
    assert_eq!(dismissed, Some(Ok(PromptOutcome::Dismissed)));
    assert_eq!(built.parts.dialogs.requests.snapshot(), []);
}

#[test]
fn a_dialog_failure_after_fallback_is_returned() {
    let built = Built::new(PromptStyle::Notification, true, false);
    built
        .parts
        .notifications
        .push_error(BackendError::Unavailable("no server".into()));
    built
        .parts
        .dialogs
        .outcomes
        .push(Err(BackendError::Unavailable("kdialog".into())));
    let result = now_or_never(built.parts.prompter.show(request(45)));
    assert!(matches!(result, Some(Err(BackendError::Unavailable(_)))));
}

#[test]
fn reminders_stay_on_the_notification() {
    let built = Built::new(PromptStyle::Dialog, true, false);
    let reminder = Reminder::PanelCare {
        screen_on: Duration::from_hours(4),
    };
    assert_eq!(
        now_or_never(built.parts.prompter.remind(reminder)),
        Some(Ok(()))
    );
    assert_eq!(built.parts.notifications.reminders(), vec![reminder]);
    assert_eq!(built.parts.dialogs.requests.snapshot(), []);
}

#[test]
fn session_builds_without_showing_anything() {
    let history = Arc::new(MemoryHistory::new());
    let prompter = StylePrompter::session(&PromptConfig::default(), history);
    let prompt = PromptConfig {
        style: PromptStyle::Dialog,
        urgency: PromptUrgency::Low,
        ..PromptConfig::default()
    };
    prompter.apply(&prompt);
    prompter.set_urgency(PromptUrgency::Normal);
}

struct Advancing {
    clock: FakeClock,
    by: Duration,
    inner: Arc<MockPrompter>,
}

impl Prompter for Advancing {
    fn show(&self, request: PromptRequest) -> BackendFuture<'_, PromptOutcome> {
        self.clock.advance(self.by);
        self.inner.show(request)
    }

    fn dismiss(&self) -> BackendFuture<'_, ()> {
        self.inner.dismiss()
    }

    fn remind(&self, reminder: Reminder) -> BackendFuture<'_, ()> {
        self.inner.remind(reminder)
    }
}

struct Gated {
    slot: std::sync::Mutex<Option<oneshot::Sender<Result<PromptOutcome, BackendError>>>>,
    started: tokio::sync::Notify,
}

impl Gated {
    fn new() -> Self {
        Self {
            slot: std::sync::Mutex::new(None),
            started: tokio::sync::Notify::new(),
        }
    }

    fn finish(&self, result: Result<PromptOutcome, BackendError>) {
        let tx = self
            .slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
            .unwrap();
        tx.send(result).unwrap();
    }
}

impl Prompter for Gated {
    fn show(&self, _request: PromptRequest) -> BackendFuture<'_, PromptOutcome> {
        let (tx, rx) = oneshot::channel();
        *self
            .slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(tx);
        self.started.notify_one();
        Box::pin(async move { rx.await.unwrap_or(Ok(PromptOutcome::Dismissed)) })
    }

    fn dismiss(&self) -> BackendFuture<'_, ()> {
        Box::pin(std::future::ready(Ok(())))
    }

    fn remind(&self, _reminder: Reminder) -> BackendFuture<'_, ()> {
        Box::pin(std::future::ready(Ok(())))
    }
}

#[tokio::test]
async fn dismiss_does_not_fall_back() {
    let notifications = Arc::new(Gated::new());
    let dialogs = Arc::new(MockDialog::new());
    let prompter = Arc::new(StylePrompter::new(PromptParts {
        notifications: Arc::clone(&notifications) as _,
        dialogs: Arc::clone(&dialogs) as _,
        fullscreen: Arc::new(MockFullscreen::new(Ok(false))),
        history: Arc::new(MemoryHistory::new()),
        clock: Arc::new(FakeClock::new()),
        style: PromptStyle::Notification,
        fallback_to_dialog: true,
        notifications_hidden_over_fullscreen: false,
    }));
    let task = {
        let prompter = Arc::clone(&prompter);
        tokio::spawn(async move { prompter.show(request(45)).await })
    };
    notifications.started.notified().await;
    prompter.dismiss().await.unwrap();
    notifications.finish(Ok(PromptOutcome::Dismissed));
    let result = tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result, Ok(PromptOutcome::Dismissed));
    assert_eq!(dialogs.requests.snapshot(), []);
}
