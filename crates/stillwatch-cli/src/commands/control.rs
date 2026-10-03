//! `stillwatch snooze`, `cancel-snooze`, `pause`, and `resume`: one call
//! each and a one-line confirmation.

use std::future::Future;
use std::io::Write;

use stillwatch_ipc::proxy::StillwatchProxy;

use crate::args::SnoozeArgs;
use crate::connect::DaemonError;

/// `stillwatch snooze <DURATION>`
///
/// The daemon checks the length against the `[prompt]` snooze rules.
///
/// # Errors
///
/// Fails with [`DaemonError::Refused`] and the daemon's reason if the length
/// breaks the rules.
pub async fn snooze(
    proxy: &StillwatchProxy<'_>,
    args: &SnoozeArgs,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    let done = format!("snoozed for {}", humantime::format_duration(args.duration));
    confirm(proxy.snooze(args.duration.as_secs()), &done, out).await
}

/// Waits for `call` and prints `done` once it succeeds.
///
/// # Errors
///
/// Fails if the call does, mapped to a [`DaemonError`], or if `out` can't be
/// written.
pub async fn confirm(
    call: impl Future<Output = zbus::Result<()>>,
    done: &str,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    call.await.map_err(DaemonError::from)?;
    writeln!(out, "{done}")?;
    Ok(())
}
