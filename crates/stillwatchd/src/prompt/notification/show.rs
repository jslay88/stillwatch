//! One prompt: send it, keep its countdown current, and wait for the answer.

use std::time::Duration;

use futures_util::StreamExt as _;
use stillwatch_core::backend::BackendError;
use stillwatch_core::config::PromptUrgency;
use stillwatch_core::prompt::{PromptOutcome, PromptRequest};
use tokio::time::{Instant, interval_at};

use super::actions;
use super::countdown::Countdown;
use super::message::Message;
use super::open::OpenSlot;
use super::proxy::{self, NotificationsProxy, unavailable};
use super::text;

/// Everything one prompt needs from the prompter.
pub(crate) struct Prompt<'a> {
    pub(crate) proxy: NotificationsProxy<'static>,
    pub(crate) open: &'a OpenSlot,
    pub(crate) urgency: PromptUrgency,
    /// Real time between countdown updates.
    pub(crate) tick: Duration,
}

impl Prompt<'_> {
    /// Shows `request` and resolves with the user's answer. The notification
    /// is closed on return, and when the future is dropped.
    pub(crate) async fn run(self, request: &PromptRequest) -> Result<PromptOutcome, BackendError> {
        let proxy = &self.proxy;
        proxy::require_actions(proxy).await?;
        // Subscribe before Notify so an instant answer can't be missed.
        let mut invoked = proxy.receive_action_invoked().await.map_err(unavailable)?;
        let mut closed = proxy
            .receive_notification_closed()
            .await
            .map_err(unavailable)?;
        self.open.close_current().await;

        let mut countdown = Countdown::new(request.countdown);
        let mut message = Message::prompt(request, countdown.remaining(), self.urgency);
        let mut id = message.send(proxy, 0).await.map_err(unavailable)?;
        let guard = self.open.track(proxy.clone(), id);
        let mut updates = interval_at(Instant::now() + self.tick, self.tick);
        let mut counting = true;
        let conn = proxy.inner().connection().clone();
        let vanished = crate::peer::until_replaced(&conn, "org.freedesktop.Notifications");
        tokio::pin!(vanished);
        loop {
            tokio::select! {
                signal = invoked.next() => {
                    let signal = signal.ok_or_else(signals_ended)?;
                    let Ok(args) = signal.args() else { continue };
                    if args.id != id {
                        continue;
                    }
                    if let Some(outcome) = actions::outcome(&args.action_key) {
                        guard.close().await;
                        return Ok(outcome);
                    }
                }
                signal = closed.next() => {
                    let signal = signal.ok_or_else(signals_ended)?;
                    if signal.args().is_ok_and(|args| args.id == id) {
                        guard.forget();
                        return Ok(PromptOutcome::Dismissed);
                    }
                }
                _ = updates.tick(), if counting => {
                    counting = countdown.advance();
                    if !counting {
                        continue;
                    }
                    message.summary = text::prompt_summary(countdown.remaining());
                    id = message.send(proxy, id).await.map_err(unavailable)?;
                    if !guard.set_id(id) {
                        // Dismissed while the update was in flight, which
                        // may have re-opened it under a new id.
                        proxy::close(proxy, id).await;
                        return Ok(PromptOutcome::Dismissed);
                    }
                }
                result = &mut vanished => {
                    result?;
                    return Err(BackendError::Disconnected(
                        "notification server vanished".into(),
                    ));
                }
            }
        }
    }
}

fn signals_ended() -> BackendError {
    BackendError::Disconnected("notification signals ended".into())
}
