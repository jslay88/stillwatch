//! `History(since)` when the daemon is up, otherwise the ring file.
//!
//! The bus has no history-added signal, so the page polls [`fetch`] while it
//! is showing. This does not add a bus method.

use std::path::{Path, PathBuf};

use stillwatch_core::history::HistoryEntry;
use stillwatch_ipc::json::{from_json, from_json_lines};
use stillwatch_ipc::proxy::StillwatchProxy;
use tokio::sync::mpsc;

use super::detail::rows_from;
use super::{HistoryLoad, HistorySource};
use crate::shell::DaemonEvent;

/// `$XDG_STATE_HOME/stillwatch/history.jsonl`.
///
/// # Errors
///
/// Returns the paths error when no state directory can be resolved.
pub(crate) fn default_history_path() -> Result<PathBuf, String> {
    stillwatch_ipc::paths::state_dir()
        .map(|dir| dir.join("history.jsonl"))
        .map_err(|err| err.to_string())
}

/// Reads `path`, skipping a torn or unreadable line.
///
/// A missing file is an empty history, not an error.
///
/// # Errors
///
/// Returns the I/O error when the file exists but can't be read.
pub(crate) fn read_history(path: &Path) -> Result<Vec<HistoryEntry>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err.to_string()),
    };
    let mut entries = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        match from_json::<HistoryEntry>(line) {
            Ok(entry) => entries.push(entry),
            Err(err) => {
                tracing::warn!(line = index + 1, %err, "skipping a history line");
            }
        }
    }
    Ok(entries)
}

/// Loads history for the page and sends one [`DaemonEvent::History`].
pub(crate) async fn fetch(
    proxy: Option<&StillwatchProxy<'_>>,
    since_seconds: u64,
    events: &mpsc::Sender<DaemonEvent>,
) {
    let path = match default_history_path() {
        Ok(path) => path,
        Err(err) => {
            let _ = events.send(DaemonEvent::History(Err(err))).await;
            return;
        }
    };
    let loaded = load(proxy, since_seconds, &path).await;
    let _ = events.send(DaemonEvent::History(loaded)).await;
}

/// Daemon reply, or the file at `path` when `proxy` is `None`.
///
/// # Errors
///
/// Returns the D-Bus or file error. A missing file is an empty file load.
pub(crate) async fn load(
    proxy: Option<&StillwatchProxy<'_>>,
    since_seconds: u64,
    path: &Path,
) -> Result<HistoryLoad, String> {
    if let Some(proxy) = proxy {
        return daemon_history(proxy, since_seconds).await;
    }
    let path = path.to_path_buf();
    let entries = tokio::task::spawn_blocking(move || read_history(&path))
        .await
        .map_err(|err| err.to_string())??;
    Ok(HistoryLoad {
        rows: rows_from(&entries),
        source: HistorySource::File,
    })
}

async fn daemon_history(
    proxy: &StillwatchProxy<'_>,
    since_seconds: u64,
) -> Result<HistoryLoad, String> {
    let body = proxy.history(since_seconds).await.map_err(|err| {
        tracing::warn!(%err, "history");
        err.to_string()
    })?;
    let entries = from_json_lines::<HistoryEntry>(&body).map_err(|err| err.to_string())?;
    Ok(HistoryLoad {
        rows: rows_from(&entries),
        source: HistorySource::Daemon,
    })
}
