//! `stillwatch config` commands, which work on the file directly.

use super::unavailable;
use crate::args::{ConfigCheckArgs, ConfigInitArgs};

/// `stillwatch config init`
///
/// # Errors
///
/// Not implemented yet; always fails.
pub fn init(args: &ConfigInitArgs) -> anyhow::Result<()> {
    tracing::debug!(?args, "config init");
    Err(unavailable("config init"))
}

/// `stillwatch config check`
///
/// # Errors
///
/// Not implemented yet; always fails.
pub fn check(args: &ConfigCheckArgs) -> anyhow::Result<()> {
    tracing::debug!(?args, "config check");
    Err(unavailable("config check"))
}
