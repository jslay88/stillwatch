//! The `org.freedesktop.Notifications` client.

use std::fmt::Display;

use stillwatch_core::backend::BackendError;

/// The capability a server needs for the prompt's buttons.
const ACTIONS_CAPABILITY: &str = "actions";

/// `org.freedesktop.Notifications`, minus `Notify`: its eight arguments are
/// sent as one tuple by [`Message::send`](super::message::Message::send).
#[zbus::proxy(
    interface = "org.freedesktop.Notifications",
    default_service = "org.freedesktop.Notifications",
    default_path = "/org/freedesktop/Notifications",
    gen_blocking = false
)]
pub(crate) trait Notifications {
    /// The optional spec features the server supports.
    fn get_capabilities(&self) -> zbus::Result<Vec<String>>;

    /// Closes notification `id`.
    fn close_notification(&self, id: u32) -> zbus::Result<()>;

    /// The user picked action `action_key` on notification `id`.
    #[zbus(signal)]
    fn action_invoked(&self, id: u32, action_key: String) -> zbus::Result<()>;

    /// Notification `id` went away: 1 expired, 2 dismissed by the user,
    /// 3 closed by `CloseNotification`, 4 undefined.
    #[zbus(signal)]
    fn notification_closed(&self, id: u32, reason: u32) -> zbus::Result<()>;
}

/// A failed call to the server. Reported as unavailable so the state machine
/// can fall back to the dialog.
pub(crate) fn unavailable(err: impl Display) -> BackendError {
    BackendError::Unavailable(format!("notification server: {err}"))
}

/// Fails unless the server is reachable and can show action buttons.
pub(crate) async fn require_actions(proxy: &NotificationsProxy<'_>) -> Result<(), BackendError> {
    let capabilities = proxy.get_capabilities().await.map_err(unavailable)?;
    if capabilities.iter().any(|cap| cap == ACTIONS_CAPABILITY) {
        Ok(())
    } else {
        Err(BackendError::Unsupported(
            "the notification server doesn't support actions".into(),
        ))
    }
}

/// Closes notification `id`. A server that already dropped it answers with an
/// error, which is fine.
pub(crate) async fn close(proxy: &NotificationsProxy<'_>, id: u32) {
    if let Err(err) = proxy.close_notification(id).await {
        tracing::debug!(id, %err, "closing the notification failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call_failures_are_unavailable() {
        let err = unavailable(zbus::Error::Failure("no server".into()));
        assert!(
            matches!(err, BackendError::Unavailable(ref detail) if detail.contains("no server"))
        );
    }
}
