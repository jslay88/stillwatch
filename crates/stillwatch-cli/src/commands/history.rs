//! `stillwatch history`

use std::io::Write;
use std::time::Duration;

use stillwatch_core::history::HistoryEntry;
use stillwatch_ipc::json::{from_json_lines, to_json_lines};
use stillwatch_ipc::proxy::StillwatchProxy;

use crate::args::HistoryArgs;
use crate::connect::DaemonError;
use crate::render::{self, Style};

/// Prints history entries, oldest first, as a table or as JSON lines of
/// `HistoryEntry`.
///
/// # Errors
///
/// Fails if the daemon can't be asked or `out` can't be written.
pub async fn run(
    proxy: &StillwatchProxy<'_>,
    args: &HistoryArgs,
    style: &Style,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    let lines = proxy
        .history(since_seconds(args.since))
        .await
        .map_err(DaemonError::from)?;
    let entries: Vec<HistoryEntry> = from_json_lines(&lines).map_err(DaemonError::from)?;
    let text = if args.json {
        to_json_lines(&entries)?
    } else {
        render::history::render(&entries, style)
    };
    out.write_all(text.as_bytes())?;
    Ok(())
}

/// The `History(since_seconds)` argument, where 0 means everything. A
/// `--since` under a second still asks for the last second.
fn since_seconds(since: Option<Duration>) -> u64 {
    since.map_or(0, |since| since.as_secs().max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn since_rounds_to_whole_seconds_but_never_to_all() {
        assert_eq!(since_seconds(None), 0);
        assert_eq!(since_seconds(Some(Duration::from_hours(2))), 7200);
        assert_eq!(since_seconds(Some(Duration::from_millis(400))), 1);
    }
}
