//! The ring file itself. Everything here does blocking I/O, so
//! [`HistoryRing`](super::HistoryRing) runs it on the blocking pool.

use std::fs::{self, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use jiff::Timestamp;
use stillwatch_core::backend::BackendError;
use stillwatch_core::config::HistoryConfig;
use stillwatch_core::history::HistoryEntry;
use stillwatch_ipc::json::{from_json, to_json, to_json_lines};

use super::TRIM_MARGIN_PERCENT;

/// Extra lines allowed past `max_entries` before the file is rewritten. With
/// the default 1000 entries the file is rewritten once every 100 appends
/// instead of on every append.
pub(super) fn margin(max_entries: usize) -> usize {
    (max_entries.saturating_mul(TRIM_MARGIN_PERCENT) / 100).max(1)
}

pub(super) struct Ring {
    path: PathBuf,
    enabled: bool,
    max_entries: usize,
    /// Lines in the file, counted on first use.
    lines: Option<usize>,
    /// The file ends in a partial line (a crash mid-append), so the next
    /// append starts a fresh line instead of gluing onto it.
    partial_tail: bool,
}

impl Ring {
    pub(super) fn new(path: PathBuf, config: &HistoryConfig) -> Self {
        let mut ring = Self {
            path,
            enabled: true,
            max_entries: 1,
            lines: None,
            partial_tail: false,
        };
        ring.configure(config);
        ring
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn configure(&mut self, config: &HistoryConfig) {
        self.enabled = config.enabled;
        self.max_entries = usize::try_from(config.max_entries)
            .unwrap_or(usize::MAX)
            .max(1);
    }

    pub(super) fn record(&mut self, entry: &HistoryEntry) -> Result<(), BackendError> {
        if !self.enabled {
            return Ok(());
        }
        let json = to_json(entry).map_err(|err| BackendError::Protocol(err.to_string()))?;
        let lines = self.count()?;
        let mut line = String::with_capacity(json.len() + 2);
        if self.partial_tail {
            line.push('\n');
        }
        line.push_str(&json);
        line.push('\n');
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?
            .write_all(line.as_bytes())?;
        self.partial_tail = false;
        self.lines = Some(lines.saturating_add(1));
        self.trim_if_needed()
    }

    /// Rewrites the file down to `max_entries` once it holds more than
    /// `max_entries` plus the [`margin`].
    pub(super) fn trim_if_needed(&mut self) -> Result<(), BackendError> {
        if !self.enabled {
            return Ok(());
        }
        let limit = self.max_entries.saturating_add(margin(self.max_entries));
        if self.count()? <= limit {
            return Ok(());
        }
        let entries = self.read_all()?;
        let keep = &entries[entries.len().saturating_sub(self.max_entries)..];
        let contents =
            to_json_lines(keep).map_err(|err| BackendError::Protocol(err.to_string()))?;
        let mut temp = self.path.clone().into_os_string();
        temp.push(".tmp");
        fs::write(&temp, contents)?;
        fs::rename(&temp, &self.path)?;
        self.lines = Some(keep.len());
        self.partial_tail = false;
        Ok(())
    }

    /// The newest `max_entries` entries with `at >= since`, oldest first.
    pub(super) fn read(&self, since: Timestamp) -> Result<Vec<HistoryEntry>, BackendError> {
        let entries = self.read_all()?;
        let skip = entries.len().saturating_sub(self.max_entries);
        Ok(entries
            .into_iter()
            .skip(skip)
            .filter(|entry| entry.at >= since)
            .collect())
    }

    fn read_all(&self) -> Result<Vec<HistoryEntry>, BackendError> {
        Ok(read_file(&self.path)?.map_or_else(Vec::new, |bytes| parse(&bytes)))
    }

    /// Counts the lines already on disk the first time, creating the
    /// directory so the first append can't fail on it.
    fn count(&mut self) -> Result<usize, BackendError> {
        if let Some(lines) = self.lines {
            return Ok(lines);
        }
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let bytes = read_file(&self.path)?.unwrap_or_default();
        let lines = bytes
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .count();
        self.partial_tail = bytes.last().is_some_and(|byte| *byte != b'\n');
        self.lines = Some(lines);
        Ok(lines)
    }
}

fn read_file(path: &Path) -> io::Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

/// Decodes JSON lines, skipping (with a warning) any line that isn't an
/// entry, such as one cut short by a crash.
pub(super) fn parse(bytes: &[u8]) -> Vec<HistoryEntry> {
    String::from_utf8_lossy(bytes)
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .filter_map(|(index, line)| match from_json(line) {
            Ok(entry) => Some(entry),
            Err(error) => {
                tracing::warn!(line = index + 1, %error, "skipping unreadable history line");
                None
            }
        })
        .collect()
}
