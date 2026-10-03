use jiff::Timestamp;

use super::BackendFuture;
use crate::history::HistoryEntry;

/// Persistent storage for decision history.
pub trait HistorySink: Send + Sync {
    /// Appends an entry. Implementations may drop the oldest entries to stay
    /// within `history.max_entries`.
    fn record(&self, entry: HistoryEntry) -> BackendFuture<'_, ()>;

    /// Entries with `at >= since`, oldest first.
    fn read(&self, since: Timestamp) -> BackendFuture<'_, Vec<HistoryEntry>>;
}
