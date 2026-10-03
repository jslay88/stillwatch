//! Commands that talk to the running daemon over D-Bus.

use super::unavailable;
use crate::args::{HistoryArgs, SnoozeArgs, StatusArgs};

/// `stillwatch status`
///
/// # Errors
///
/// Not implemented yet; always fails.
pub fn status(args: &StatusArgs) -> anyhow::Result<()> {
    tracing::debug!(?args, "status");
    Err(unavailable("status"))
}

/// `stillwatch snooze <DURATION>`
///
/// # Errors
///
/// Not implemented yet; always fails.
pub fn snooze(args: &SnoozeArgs) -> anyhow::Result<()> {
    tracing::debug!(?args, "snooze");
    Err(unavailable("snooze"))
}

/// `stillwatch cancel-snooze`
///
/// # Errors
///
/// Not implemented yet; always fails.
pub fn cancel_snooze() -> anyhow::Result<()> {
    Err(unavailable("cancel-snooze"))
}

/// `stillwatch pause`
///
/// # Errors
///
/// Not implemented yet; always fails.
pub fn pause() -> anyhow::Result<()> {
    Err(unavailable("pause"))
}

/// `stillwatch resume`
///
/// # Errors
///
/// Not implemented yet; always fails.
pub fn resume() -> anyhow::Result<()> {
    Err(unavailable("resume"))
}

/// `stillwatch reload`
///
/// # Errors
///
/// Not implemented yet; always fails.
pub fn reload() -> anyhow::Result<()> {
    Err(unavailable("reload"))
}

/// `stillwatch history`
///
/// # Errors
///
/// Not implemented yet; always fails.
pub fn history(args: &HistoryArgs) -> anyhow::Result<()> {
    tracing::debug!(?args, "history");
    Err(unavailable("history"))
}
