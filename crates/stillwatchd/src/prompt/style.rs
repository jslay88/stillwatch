//! [`StylePrompter`]: notification, dialog, or a fallback for the time left.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use stillwatch_core::backend::{BackendError, BackendFuture, HistorySink, Prompter};
use stillwatch_core::config::{PromptConfig, PromptStyle, PromptUrgency};
use stillwatch_core::history::{HistoryEntry, HistoryKind, PromptMedium, PromptReason};
use stillwatch_core::prompt::{PromptOutcome, PromptRequest, Reminder};
use stillwatch_core::time::{Clock, SystemClock};

use super::dialog::{DialogLauncher, KdialogLauncher};
use super::fullscreen::{FullscreenMonitor, UnverifiedFullscreen};
use super::notification::NotificationPrompter;
use super::select::{self, Choice, NOTIFICATIONS_HIDDEN_OVER_FULLSCREEN, PromptFacts};
use crate::process::TokioRunner;

/// Pieces a [`StylePrompter`] is built from. Tests pass mocks.
pub struct PromptParts {
    /// Notification prompter. `show` failing or returning
    /// [`PromptOutcome::Dismissed`] can fall back to the dialog.
    pub notifications: Arc<dyn Prompter>,
    /// Dialog. Tests pass a mock; the daemon passes [`KdialogLauncher`].
    pub dialogs: Arc<dyn DialogLauncher>,
    /// Fullscreen check. Consulted only when notifications are known to be
    /// hidden by a fullscreen surface.
    pub fullscreen: Arc<dyn FullscreenMonitor>,
    /// Where the chosen style and the reason are recorded.
    pub history: Arc<dyn HistorySink>,
    /// Elapsed time for the remaining countdown.
    pub clock: Arc<dyn Clock>,
    /// `prompt.style`.
    pub style: PromptStyle,
    /// `prompt.fallback_to_dialog`.
    pub fallback_to_dialog: bool,
    /// See [`NOTIFICATIONS_HIDDEN_OVER_FULLSCREEN`].
    pub notifications_hidden_over_fullscreen: bool,
}

#[derive(Clone, Copy)]
struct Settings {
    style: PromptStyle,
    fallback_to_dialog: bool,
    notifications_hidden_over_fullscreen: bool,
}

/// Picks a notification or a dialog, and falls back without resetting the
/// countdown.
///
/// The state machine's prompt timer keeps running. A fallback dialog is
/// given `request.countdown` minus the time since `show` started.
///
/// A newer `show`, or [`dismiss`](Prompter::dismiss), cancels the one in
/// flight. That close is not a user dismissal, so it does not fall back.
pub struct StylePrompter {
    notifications: Arc<dyn Prompter>,
    dialogs: Arc<dyn DialogLauncher>,
    fullscreen: Arc<dyn FullscreenMonitor>,
    history: Arc<dyn HistorySink>,
    clock: Arc<dyn Clock>,
    /// Set when `notifications` is a [`NotificationPrompter`], so a reload
    /// can change urgency.
    urgency: Option<Arc<NotificationPrompter>>,
    settings: Mutex<Settings>,
    /// Bumped by each `show` and by `dismiss`. An in-flight show whose token
    /// no longer matches was replaced or dismissed and must not fall back.
    epoch: AtomicU64,
}

impl StylePrompter {
    /// Builds a prompter from `parts`.
    #[must_use]
    pub fn new(parts: PromptParts) -> Self {
        Self {
            notifications: parts.notifications,
            dialogs: parts.dialogs,
            fullscreen: parts.fullscreen,
            history: parts.history,
            clock: parts.clock,
            urgency: None,
            settings: Mutex::new(Settings {
                style: parts.style,
                fallback_to_dialog: parts.fallback_to_dialog,
                notifications_hidden_over_fullscreen: parts.notifications_hidden_over_fullscreen,
            }),
            epoch: AtomicU64::new(0),
        }
    }

    /// Notification on the session bus, interim `kdialog`, and no fullscreen
    /// probe. `auto` uses the notification.
    #[must_use]
    pub fn session(prompt: &PromptConfig, history: Arc<dyn HistorySink>) -> Self {
        let notifications = Arc::new(NotificationPrompter::session(prompt.urgency));
        let mut prompter = Self::new(PromptParts {
            notifications: Arc::clone(&notifications) as Arc<dyn Prompter>,
            dialogs: Arc::new(KdialogLauncher::new(Arc::new(TokioRunner))),
            fullscreen: Arc::new(UnverifiedFullscreen),
            history,
            clock: Arc::new(SystemClock),
            style: prompt.style,
            fallback_to_dialog: prompt.fallback_to_dialog,
            notifications_hidden_over_fullscreen: NOTIFICATIONS_HIDDEN_OVER_FULLSCREEN,
        });
        prompter.urgency = Some(notifications);
        prompter
    }

    /// Applies a reloaded `[prompt]` section. The verified-fullscreen flag
    /// is not a setting.
    pub fn apply(&self, prompt: &PromptConfig) {
        let mut settings = lock(&self.settings);
        settings.style = prompt.style;
        settings.fallback_to_dialog = prompt.fallback_to_dialog;
        drop(settings);
        self.set_urgency(prompt.urgency);
    }

    /// Changes notification urgency when this prompter owns a
    /// [`NotificationPrompter`].
    pub fn set_urgency(&self, urgency: PromptUrgency) {
        if let Some(notifications) = &self.urgency {
            notifications.set_urgency(urgency);
        }
    }

    fn settings(&self) -> Settings {
        *lock(&self.settings)
    }

    fn superseded(&self, epoch: u64) -> bool {
        self.epoch.load(Ordering::SeqCst) != epoch
    }

    async fn choose(&self) -> Choice {
        let settings = self.settings();
        let mut facts = PromptFacts {
            fullscreen: false,
            notifications_hidden_over_fullscreen: settings.notifications_hidden_over_fullscreen,
        };
        if settings.style == PromptStyle::Auto && settings.notifications_hidden_over_fullscreen {
            facts.fullscreen = self.fullscreen_active().await;
        }
        select::select(settings.style, facts)
    }

    async fn fullscreen_active(&self) -> bool {
        match self.fullscreen.is_active().await {
            Ok(active) => active,
            Err(error) => {
                tracing::warn!(%error, "fullscreen check failed; using the notification");
                false
            }
        }
    }

    async fn record(&self, choice: Choice) {
        let entry = HistoryEntry::new(self.clock.wall_now(), HistoryKind::Prompt)
            .with_prompt(choice.medium, choice.reason);
        if let Err(error) = self.history.record(entry).await {
            tracing::warn!(%error, "could not record the prompt style");
        }
    }

    async fn after_notification(
        &self,
        request: PromptRequest,
        started: Instant,
        epoch: u64,
    ) -> Result<PromptOutcome, BackendError> {
        let result = self.notifications.show(request.clone()).await;
        if self.superseded(epoch) || !self.settings().fallback_to_dialog {
            return result;
        }
        match result {
            Ok(PromptOutcome::Dismissed) => {
                self.show_dialog(request, started, PromptReason::FallbackClosed)
                    .await
            }
            Err(error) => {
                let reason = fallback_reason(&error);
                self.show_dialog(request, started, reason).await
            }
            Ok(outcome) => Ok(outcome),
        }
    }

    async fn show_dialog(
        &self,
        request: PromptRequest,
        started: Instant,
        reason: PromptReason,
    ) -> Result<PromptOutcome, BackendError> {
        self.record(Choice {
            medium: PromptMedium::Dialog,
            reason,
        })
        .await;
        let request = with_remaining(request, started, self.clock.as_ref());
        self.dialogs.launch(request).await
    }
}

impl Prompter for StylePrompter {
    fn show(&self, request: PromptRequest) -> BackendFuture<'_, PromptOutcome> {
        Box::pin(async move {
            let epoch = self.epoch.fetch_add(1, Ordering::SeqCst) + 1;
            let _ = self.dialogs.dismiss().await;
            let started = self.clock.now();
            let choice = self.choose().await;
            match choice.medium {
                PromptMedium::Notification => {
                    self.record(choice).await;
                    self.after_notification(request, started, epoch).await
                }
                PromptMedium::Dialog => self.show_dialog(request, started, choice.reason).await,
            }
        })
    }

    fn dismiss(&self) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            self.epoch.fetch_add(1, Ordering::SeqCst);
            self.notifications.dismiss().await?;
            self.dialogs.dismiss().await?;
            Ok(())
        })
    }

    fn remind(&self, reminder: Reminder) -> BackendFuture<'_, ()> {
        self.notifications.remind(reminder)
    }
}

fn with_remaining(
    mut request: PromptRequest,
    started: Instant,
    clock: &dyn Clock,
) -> PromptRequest {
    let elapsed = clock.now().saturating_duration_since(started);
    request.countdown = request.countdown.saturating_sub(elapsed);
    request
}

/// A missing server and a failing `Notify` are both
/// [`BackendError::Unavailable`] (the notification client reports them that
/// way). Anything else, such as a server without actions, is a failed show.
fn fallback_reason(error: &BackendError) -> PromptReason {
    match error {
        BackendError::Unavailable(_) => PromptReason::FallbackUnavailable,
        _ => PromptReason::FallbackFailed,
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests;
