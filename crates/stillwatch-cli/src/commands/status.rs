//! `stillwatch status`

use std::io::Write;

use stillwatch_ipc::json::{from_json, to_json};
use stillwatch_ipc::proxy::StillwatchProxy;
use stillwatch_ipc::status::StatusPayload;

use crate::args::StatusArgs;
use crate::connect::DaemonError;
use crate::render::{self, Style};

/// Prints the daemon's status, as text or as one JSON `StatusPayload`.
///
/// # Errors
///
/// Fails if the daemon can't be asked or `out` can't be written.
pub async fn run(
    proxy: &StillwatchProxy<'_>,
    args: &StatusArgs,
    style: &Style,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    let json = proxy.status().await.map_err(DaemonError::from)?;
    let status: StatusPayload = from_json(&json).map_err(DaemonError::from)?;
    if args.json {
        writeln!(out, "{}", to_json(&status)?)?;
    } else {
        out.write_all(render::status::render(&status, style).as_bytes())?;
    }
    Ok(())
}
