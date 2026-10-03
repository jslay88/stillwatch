use std::collections::VecDeque;
use std::sync::Mutex;

use jiff::Timestamp;

use crate::backend::{BackendFuture, HistorySink};
use crate::history::HistoryEntry;
use crate::sync::lock;

/// An in-memory [`HistorySink`], optionally capped like the on-disk ring.
#[derive(Debug, Default)]
pub struct MemoryHistory {
    entries: Mutex<VecDeque<HistoryEntry>>,
    capacity: Option<usize>,
}

impl MemoryHistory {
    /// An unbounded history.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A history that keeps only the newest `capacity` entries.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: Mutex::default(),
            capacity: Some(capacity),
        }
    }

    /// Every stored entry, oldest first.
    #[must_use]
    pub fn entries(&self) -> Vec<HistoryEntry> {
        lock(&self.entries).iter().cloned().collect()
    }
}

impl HistorySink for MemoryHistory {
    fn record(&self, entry: HistoryEntry) -> BackendFuture<'_, ()> {
        let mut entries = lock(&self.entries);
        entries.push_back(entry);
        if let Some(capacity) = self.capacity {
            while entries.len() > capacity {
                entries.pop_front();
            }
        }
        Box::pin(std::future::ready(Ok(())))
    }

    fn read(&self, since: Timestamp) -> BackendFuture<'_, Vec<HistoryEntry>> {
        let matching = lock(&self.entries)
            .iter()
            .filter(|entry| entry.at >= since)
            .cloned()
            .collect();
        Box::pin(std::future::ready(Ok(matching)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::HistoryKind;
    use crate::mocks::now_or_never;

    fn entry(second: i64) -> HistoryEntry {
        HistoryEntry::new(Timestamp::from_second(second).unwrap(), HistoryKind::Prompt)
    }

    #[test]
    fn read_filters_by_timestamp() {
        let history = MemoryHistory::new();
        for second in [10, 20, 30] {
            assert_eq!(now_or_never(history.record(entry(second))), Some(Ok(())));
        }
        let since = Timestamp::from_second(20).unwrap();
        assert_eq!(
            now_or_never(history.read(since)),
            Some(Ok(vec![entry(20), entry(30)]))
        );
        assert_eq!(history.entries().len(), 3);
    }

    #[test]
    fn capacity_drops_the_oldest() {
        let history = MemoryHistory::with_capacity(2);
        for second in [1, 2, 3] {
            let _ = now_or_never(history.record(entry(second)));
        }
        assert_eq!(history.entries(), vec![entry(2), entry(3)]);
    }
}
