//! Panel care counters on disk: `panel.json` in the state directory.
//!
//! Writes are debounced. [`PanelStore::flush`] writes immediately, which the
//! daemon loop should call on shutdown. A missing file starts at zero. A
//! file that isn't JSON is ignored and logged; the next real change replaces
//! it.

#[cfg(test)]
mod tests;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use stillwatch_core::panel::PanelRecord;
use stillwatch_ipc::json::{from_json, to_json};
use stillwatch_ipc::paths::{self, PathsError};

/// File name inside the state directory.
pub const FILE_NAME: &str = "panel.json";

/// Quiet period after the last change before it is written.
pub const WRITE_DEBOUNCE: Duration = Duration::from_secs(1);

/// `$XDG_STATE_HOME/stillwatch/panel.json`, normally
/// `~/.local/state/stillwatch/panel.json`.
///
/// # Errors
///
/// Returns [`PathsError::NoStateDir`] if no state directory can be resolved.
pub fn default_path() -> Result<PathBuf, PathsError> {
    Ok(paths::state_dir()?.join(FILE_NAME))
}

/// Why a panel care file couldn't be written.
#[derive(Debug, thiserror::Error)]
pub enum PanelError {
    /// The directory or the file couldn't be written.
    #[error("can't write panel care state: {0}")]
    Io(#[from] io::Error),
    /// The counters couldn't be encoded.
    #[error("can't encode panel care state: {0}")]
    Encode(String),
}

/// Debounced writer for [`PanelRecord`].
#[derive(Debug)]
pub struct PanelStore {
    path: PathBuf,
    debounce: Duration,
    pending: Option<PanelRecord>,
    since: Option<Instant>,
    /// Last record known to match the file, so an unchanged total isn't rewritten.
    written: Option<PanelRecord>,
}

impl PanelStore {
    /// A store at `path` with [`WRITE_DEBOUNCE`]. Nothing is read until
    /// [`load`](Self::load).
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self::with_debounce(path, WRITE_DEBOUNCE)
    }

    /// A store that writes `debounce` after the last [`update`](Self::update).
    #[must_use]
    pub fn with_debounce(path: impl Into<PathBuf>, debounce: Duration) -> Self {
        Self {
            path: path.into(),
            debounce,
            pending: None,
            since: None,
            written: None,
        }
    }

    /// The file this store writes.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Reads the file. Missing and unreadable JSON both become a zero record.
    /// A missing file is remembered as already written, so startup doesn't
    /// create `panel.json` until the counters change.
    pub fn load(&mut self) -> PanelRecord {
        match read_record(&self.path) {
            Read::Record(record) => {
                self.written = Some(record);
                record
            }
            Read::Missing => {
                let record = PanelRecord::default();
                self.written = Some(record);
                record
            }
            Read::Corrupt(error) => {
                tracing::warn!(path = %self.path.display(), %error, "ignoring unreadable panel care file");
                PanelRecord::default()
            }
        }
    }

    /// Remembers `record` if it differs from what is queued or already on disk.
    /// Each change restarts the debounce.
    pub fn update(&mut self, record: PanelRecord, now: Instant) {
        if self.pending.as_ref() == Some(&record)
            || self.pending.is_none() && self.written == Some(record)
        {
            return;
        }
        self.pending = Some(record);
        self.since = Some(now);
    }

    /// Writes the pending record once `now` is `debounce` past the last change.
    ///
    /// # Errors
    ///
    /// Returns the I/O or encode error. The record stays pending.
    pub fn flush_if_due(&mut self, now: Instant) -> Result<(), PanelError> {
        let due = self
            .since
            .is_some_and(|since| now.saturating_duration_since(since) >= self.debounce);
        if due {
            self.flush()?;
        }
        Ok(())
    }

    /// Writes a pending record immediately. Does nothing when nothing changed.
    ///
    /// # Errors
    ///
    /// Returns the I/O or encode error. The record stays pending.
    pub fn flush(&mut self) -> Result<(), PanelError> {
        let Some(record) = self.pending.take() else {
            return Ok(());
        };
        self.since = None;
        if let Err(error) = write_record(&self.path, &record) {
            self.pending = Some(record);
            return Err(error);
        }
        self.written = Some(record);
        Ok(())
    }
}

enum Read {
    Record(PanelRecord),
    Missing,
    Corrupt(String),
}

fn read_record(path: &Path) -> Read {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Read::Missing,
        Err(error) => return Read::Corrupt(error.to_string()),
    };
    let text = String::from_utf8_lossy(&bytes);
    match from_json::<PanelRecord>(&text) {
        Ok(record) => Read::Record(record),
        Err(error) => Read::Corrupt(error.to_string()),
    }
}

fn write_record(path: &Path, record: &PanelRecord) -> Result<(), PanelError> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut body = to_json(record).map_err(|error| PanelError::Encode(error.to_string()))?;
    body.push('\n');
    let mut temp = path.as_os_str().to_owned();
    temp.push(".tmp");
    let temp = PathBuf::from(temp);
    fs::write(&temp, body)?;
    fs::rename(&temp, path)?;
    Ok(())
}
