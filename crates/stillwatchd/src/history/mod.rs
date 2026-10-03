//! The decision history ring file, `history.jsonl` in the state directory.
//!
//! One JSON entry per line, appended as decisions happen. The file holds at
//! most `history.max_entries` lines after a trim; between trims it may grow
//! by a small margin (see [`TRIM_MARGIN_PERCENT`]) so appends don't rewrite
//! the whole file. A line cut short by a crash is skipped with a warning and
//! dropped at the next trim.

mod ring;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use jiff::Timestamp;
use stillwatch_core::backend::{BackendError, BackendFuture, HistorySink};
use stillwatch_core::config::HistoryConfig;
use stillwatch_core::history::HistoryEntry;
use stillwatch_ipc::paths::{self, PathsError};

use ring::Ring;

/// File name of the ring inside the state directory.
pub const FILE_NAME: &str = "history.jsonl";

/// How far past `max_entries` the file may grow before it's rewritten, as a
/// percent of `max_entries` (at least one line).
pub const TRIM_MARGIN_PERCENT: usize = 10;

/// `$XDG_STATE_HOME/stillwatch/history.jsonl`, normally
/// `~/.local/state/stillwatch/history.jsonl`.
///
/// # Errors
///
/// Returns [`PathsError::NoStateDir`] if no state directory can be resolved.
pub fn default_path() -> Result<PathBuf, PathsError> {
    Ok(paths::state_dir()?.join(FILE_NAME))
}

/// A [`HistorySink`] backed by the ring file.
///
/// File I/O runs on tokio's blocking pool, one operation at a time, so it
/// never stalls the runtime and appends never interleave with a trim.
#[derive(Clone)]
pub struct HistoryRing {
    ring: Arc<Mutex<Ring>>,
}

impl HistoryRing {
    /// A ring at `path` following `config`. Nothing touches the disk until
    /// the first record or read; the directory is created on first record.
    pub fn new(path: impl Into<PathBuf>, config: &HistoryConfig) -> Self {
        Self {
            ring: Arc::new(Mutex::new(Ring::new(path.into(), config))),
        }
    }

    /// A ring at [`default_path`].
    ///
    /// # Errors
    ///
    /// Returns [`PathsError::NoStateDir`] if no state directory can be resolved.
    pub fn open_default(config: &HistoryConfig) -> Result<Self, PathsError> {
        Ok(Self::new(default_path()?, config))
    }

    /// The file this ring writes.
    #[must_use]
    pub fn path(&self) -> PathBuf {
        lock(&self.ring).path().to_path_buf()
    }

    /// Switches to a reloaded `[history]` section. A smaller `max_entries`
    /// trims the file right away if it's past the margin; `enabled = false`
    /// stops writing but leaves the file readable.
    ///
    /// # Errors
    ///
    /// Returns the I/O error if the trim fails. The new settings apply anyway.
    pub async fn apply_config(&self, config: &HistoryConfig) -> Result<(), BackendError> {
        let config = config.clone();
        self.blocking(move |ring| {
            ring.configure(&config);
            ring.trim_if_needed()
        })
        .await
    }

    async fn blocking<T, F>(&self, op: F) -> Result<T, BackendError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Ring) -> Result<T, BackendError> + Send + 'static,
    {
        let ring = Arc::clone(&self.ring);
        tokio::task::spawn_blocking(move || op(&mut lock(&ring)))
            .await
            .map_err(|err| BackendError::Io(format!("history task failed: {err}")))?
    }
}

impl HistorySink for HistoryRing {
    fn record(&self, entry: HistoryEntry) -> BackendFuture<'_, ()> {
        Box::pin(self.blocking(move |ring| ring.record(&entry)))
    }

    fn read(&self, since: Timestamp) -> BackendFuture<'_, Vec<HistoryEntry>> {
        Box::pin(self.blocking(move |ring| ring.read(since)))
    }
}

fn lock(ring: &Mutex<Ring>) -> MutexGuard<'_, Ring> {
    ring.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests;
