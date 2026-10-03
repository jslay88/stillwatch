//! The burn-in prompt as a freedesktop notification.
//!
//! [`NotificationPrompter`] sends one notification with a button per snooze
//! preset, "Custom..." (when allowed), and "Blank now". It re-sends it every
//! 10 s with the same `replaces_id` so the countdown in its summary stays
//! current without stacking notifications. The state machine owns the real
//! timeout; `show` only reports what the user did.
//!
//! The prompt is resident with `expire_timeout` 0, so the server keeps it
//! until Stillwatch closes it: after an answer, on
//! [`dismiss`](Prompter::dismiss), when the next prompt replaces it, or when
//! the `show` future is dropped.
//!
//! No server on the bus, a failing `Notify`, or a server without the
//! `actions` capability makes `show` fail right away. [`super::StylePrompter`]
//! falls back to the dialog when that is configured. Every call is bounded
//! by a timeout.

mod actions;
mod countdown;
mod message;
mod open;
mod proxy;
mod show;
mod text;

use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use stillwatch_core::backend::{BackendError, BackendFuture, Prompter};
use stillwatch_core::config::PromptUrgency;
use stillwatch_core::prompt::{PromptOutcome, PromptRequest, Reminder};

use self::message::Message;
use self::open::OpenSlot;
use self::proxy::{NotificationsProxy, unavailable};
use self::show::Prompt;
use crate::dbus::{self, Bus};

/// A server that doesn't answer within this long counts as missing, so the
/// fallback isn't held up.
const CALL_TIMEOUT: Duration = Duration::from_secs(5);

/// A [`Prompter`] over `org.freedesktop.Notifications`.
///
/// Each `show` and `remind` opens its own connection, so a notification
/// server that restarts between prompts is picked up again.
#[derive(Debug)]
pub struct NotificationPrompter {
    bus: Bus,
    urgency: Mutex<PromptUrgency>,
    tick: Duration,
    open: OpenSlot,
}

impl NotificationPrompter {
    /// Prompts on the user's session bus with `urgency` (`prompt.urgency`).
    #[must_use]
    pub fn session(urgency: PromptUrgency) -> Self {
        Self::on(Bus::Session, urgency)
    }

    /// Prompts on the bus at `address`, for example a private test bus.
    #[must_use]
    pub fn at_address(address: impl Into<String>, urgency: PromptUrgency) -> Self {
        Self::on(Bus::Address(address.into()), urgency)
    }

    fn on(bus: Bus, urgency: PromptUrgency) -> Self {
        Self {
            bus,
            urgency: Mutex::new(urgency),
            tick: countdown::STEP,
            open: OpenSlot::default(),
        }
    }

    /// Waits `tick` of real time between countdown updates instead of 10 s.
    /// The shown countdown still drops by 10 s per update, so tests can run
    /// a whole countdown quickly.
    #[must_use]
    pub fn with_tick(mut self, tick: Duration) -> Self {
        self.tick = tick;
        self
    }

    /// Changes the urgency of prompts shown from now on, after a reload.
    pub fn set_urgency(&self, urgency: PromptUrgency) {
        *self.urgency.lock().unwrap_or_else(PoisonError::into_inner) = urgency;
    }

    fn urgency(&self) -> PromptUrgency {
        *self.urgency.lock().unwrap_or_else(PoisonError::into_inner)
    }

    async fn connect(&self) -> Result<NotificationsProxy<'static>, BackendError> {
        let conn = dbus::connect(&self.bus, CALL_TIMEOUT)
            .await
            .map_err(unavailable)?;
        NotificationsProxy::new(&conn).await.map_err(unavailable)
    }
}

impl Prompter for NotificationPrompter {
    fn show(&self, request: PromptRequest) -> BackendFuture<'_, PromptOutcome> {
        Box::pin(async move {
            let prompt = Prompt {
                proxy: self.connect().await?,
                open: &self.open,
                urgency: self.urgency(),
                tick: self.tick,
            };
            prompt.run(&request).await
        })
    }

    fn dismiss(&self) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            self.open.close_current().await;
            Ok(())
        })
    }

    fn remind(&self, reminder: Reminder) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            let proxy = self.connect().await?;
            Message::reminder(reminder)
                .send(&proxy, 0)
                .await
                .map_err(unavailable)?;
            Ok(())
        })
    }
}
