//! Troubleshooting commands that run without the daemon.

use super::idle_test;
use crate::args::IdleTestArgs;

/// `stillwatch idle-test`
///
/// # Errors
///
/// See [`idle_test::run`].
pub fn idle_test(args: &IdleTestArgs) -> anyhow::Result<()> {
    tracing::debug!(?args, "idle-test");
    idle_test::run(args)
}
