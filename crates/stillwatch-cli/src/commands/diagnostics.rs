//! Calibration and troubleshooting commands.

use super::unavailable;
use crate::args::{IdleTestArgs, ProbeArgs};

/// `stillwatch probe`
///
/// # Errors
///
/// Not implemented yet; always fails.
pub fn probe(args: &ProbeArgs) -> anyhow::Result<()> {
    tracing::debug!(?args, "probe");
    Err(unavailable("probe"))
}

/// `stillwatch idle-test`
///
/// # Errors
///
/// Not implemented yet; always fails.
pub fn idle_test(args: &IdleTestArgs) -> anyhow::Result<()> {
    tracing::debug!(?args, "idle-test");
    Err(unavailable("idle-test"))
}
