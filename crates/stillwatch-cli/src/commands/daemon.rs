//! Commands answered by the running daemon over D-Bus.

use std::future::{self, Future};
use std::io::Write;

use super::{control, history, probe, reload, status};
use crate::args::{HistoryArgs, ProbeArgs, SnoozeArgs, StatusArgs};
use crate::connect::connect;
use crate::render::Style;

/// A command for the daemon, with its arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request<'a> {
    /// `stillwatch status`
    Status(&'a StatusArgs),
    /// `stillwatch snooze`
    Snooze(&'a SnoozeArgs),
    /// `stillwatch cancel-snooze`
    CancelSnooze,
    /// `stillwatch pause`
    Pause,
    /// `stillwatch resume`
    Resume,
    /// `stillwatch reload`
    Reload,
    /// `stillwatch history`
    History(&'a HistoryArgs),
    /// `stillwatch probe`
    Probe(&'a ProbeArgs),
}

/// Runs `request` on a fresh async runtime, stopping a probe on Ctrl-C.
///
/// # Errors
///
/// Fails if the runtime can't start, or as [`execute`] does.
pub fn run(
    request: Request<'_>,
    bus_address: Option<&str>,
    style: &Style,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    super::block_on(execute(request, bus_address, style, out, interrupted()))
}

/// Connects to the daemon on `bus_address` (the session bus when `None`)
/// and runs `request`, writing its output to `out`. A probe runs until
/// `stop` completes or its `--count` is reached.
///
/// # Errors
///
/// Fails with a [`DaemonError`](crate::connect::DaemonError) if the daemon
/// isn't running or refuses the request, or if writing to `out` fails.
pub async fn execute(
    request: Request<'_>,
    bus_address: Option<&str>,
    style: &Style,
    out: &mut dyn Write,
    stop: impl Future<Output = ()>,
) -> anyhow::Result<()> {
    let proxy = connect(bus_address).await?;
    match request {
        Request::Status(args) => status::run(&proxy, args, style, out).await,
        Request::Snooze(args) => control::snooze(&proxy, args, out).await,
        Request::CancelSnooze => {
            control::confirm(proxy.cancel_snooze(), "snooze cancelled", out).await
        }
        Request::Pause => control::confirm(proxy.pause(), "paused", out).await,
        Request::Resume => control::confirm(proxy.resume(), "resumed", out).await,
        Request::Reload => reload::run(&proxy, out).await,
        Request::History(args) => history::run(&proxy, args, style, out).await,
        Request::Probe(args) => probe::run(&proxy, args, style, out, stop).await,
    }
}

/// Completes on Ctrl-C, or never if the handler can't be installed.
async fn interrupted() {
    if tokio::signal::ctrl_c().await.is_err() {
        future::pending::<()>().await;
    }
}
