//! `stillwatch reload`
//!
//! `Reload()` returns the same `(ok, errors)` the daemon broadcasts in
//! `ConfigChanged`, so the reply is printed directly instead of waiting for
//! the signal.

use std::io::Write;

use anyhow::bail;
use stillwatch_ipc::proxy::StillwatchProxy;

use crate::connect::DaemonError;

/// Asks the daemon to reload its config and prints the result. Problems are
/// printed one per line, like `stillwatch config check`.
///
/// # Errors
///
/// Fails if the daemon rejected the new config (it keeps the last good one),
/// can't be asked, or `out` can't be written.
pub async fn run(proxy: &StillwatchProxy<'_>, out: &mut dyn Write) -> anyhow::Result<()> {
    let (ok, errors) = proxy.reload().await.map_err(DaemonError::from)?;
    if ok {
        writeln!(out, "config reloaded")?;
        return Ok(());
    }
    for error in &errors {
        writeln!(out, "{error}")?;
    }
    let problems = match errors.len() {
        0 => "the config was rejected".to_owned(),
        1 => "the config has 1 problem".to_owned(),
        count => format!("the config has {count} problems"),
    };
    bail!("{problems}; stillwatchd kept the last good config")
}
