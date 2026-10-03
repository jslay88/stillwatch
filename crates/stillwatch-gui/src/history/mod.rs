//! History page: time range, kind, and one detail block per entry.

mod detail;
mod load;
mod view;

#[cfg(test)]
mod tests;

pub(crate) use load::fetch;
pub(crate) use view::page;

use stillwatch_core::history::HistoryKind;

use crate::shell::DaemonCall;

/// Kind labels, in pick-list order after "All".
const KINDS: [(HistoryKind, &str); 14] = [
    (HistoryKind::Transition, "Transition"),
    (HistoryKind::Prompt, "Prompt"),
    (HistoryKind::Snooze, "Snooze"),
    (HistoryKind::Blank, "Blank"),
    (HistoryKind::Reblank, "Re-blank"),
    (HistoryKind::Ceiling, "Ceiling"),
    (HistoryKind::ConfigReload, "Config reload"),
    (HistoryKind::OverlayUsed, "Overlay"),
    (HistoryKind::PromptAnswered, "Prompt answered"),
    (HistoryKind::ConfigReloadFailed, "Reload failed"),
    (HistoryKind::Migration, "Migration"),
    (HistoryKind::Reconnect, "Reconnect"),
    (HistoryKind::Hotplug, "Hotplug"),
    (HistoryKind::Backends, "Backends"),
];

/// How far back the page asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimeRange {
    /// The last hour.
    Hour,
    /// The last six hours.
    SixHours,
    /// The last day.
    #[default]
    Day,
    /// Everything in the ring.
    All,
}

impl TimeRange {
    /// Choices in the order the page lists them.
    pub const ALL: [Self; 4] = [Self::Hour, Self::SixHours, Self::Day, Self::All];

    /// Button label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Hour => "1h",
            Self::SixHours => "6h",
            Self::Day => "24h",
            Self::All => "All",
        }
    }

    /// `History(since)` argument. `0` is the whole ring.
    #[must_use]
    pub const fn since_seconds(self) -> u64 {
        match self {
            Self::Hour => 3_600,
            Self::SixHours => 6 * 3_600,
            Self::Day => 24 * 3_600,
            Self::All => 0,
        }
    }

    /// Unix seconds of the oldest entry this range includes.
    ///
    /// `None` means no lower bound.
    #[must_use]
    pub fn cutoff(self, now: i64) -> Option<i64> {
        let span = self.since_seconds();
        if span == 0 {
            None
        } else {
            Some(now.saturating_sub(i64::try_from(span).unwrap_or(i64::MAX)))
        }
    }
}

/// Kind filter. `All` keeps every kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KindFilter {
    /// Every kind.
    #[default]
    All,
    /// One [`HistoryKind`].
    Kind(HistoryKind),
}

impl KindFilter {
    /// Pick-list values, "All" first.
    #[must_use]
    pub fn choices() -> Vec<Self> {
        let mut choices = vec![Self::All];
        choices.extend(KINDS.iter().map(|(kind, _)| Self::Kind(*kind)));
        choices
    }

    /// Label on the pick list.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Kind(kind) => detail::kind_label(kind),
        }
    }
}

impl std::fmt::Display for KindFilter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.label())
    }
}

/// Where the rows on screen came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistorySource {
    /// `History(since)` on the daemon.
    Daemon,
    /// The ring file, because the daemon was down.
    File,
}

/// One formatted entry. Percentages are already text, so this stays `Eq`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryRow {
    /// `HistoryEntry::at` as unix seconds.
    pub at: i64,
    /// What was decided.
    pub kind: HistoryKind,
    /// List label: time, kind, and a state change when there is one.
    pub label: String,
    /// Detail block, one line each.
    pub detail: Vec<String>,
}

/// A successful load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryLoad {
    /// Oldest first, as the file and the daemon return them.
    pub rows: Vec<HistoryRow>,
    /// Daemon or file.
    pub source: HistorySource,
}

/// A history-page input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistMsg {
    /// Change the time range and reload.
    Range(TimeRange),
    /// Change the kind filter. The rows already loaded are enough.
    Kind(KindFilter),
    /// Select the entry at this unix second.
    Select(i64),
    /// A load finished.
    Loaded(HistoryLoad),
    /// A load failed. Previous rows stay.
    Failed(String),
}

/// Filters, rows, and the last load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryPage {
    /// Time range sent to `History(since)`.
    pub range: TimeRange,
    /// Kind filter applied on top of `range`.
    pub kind: KindFilter,
    /// Last load, unfiltered.
    pub rows: Vec<HistoryRow>,
    /// Selected entry's unix second.
    pub selected: Option<i64>,
    /// Set after a successful load.
    pub source: Option<HistorySource>,
    /// Last load error.
    pub error: Option<String>,
}

impl Default for HistoryPage {
    fn default() -> Self {
        Self {
            range: TimeRange::Day,
            kind: KindFilter::All,
            rows: Vec::new(),
            selected: None,
            source: None,
            error: None,
        }
    }
}

impl HistoryPage {
    /// `History(since)` for the current range.
    #[must_use]
    pub const fn since_seconds(&self) -> u64 {
        self.range.since_seconds()
    }

    /// Rows that pass the filters, newest first.
    #[must_use]
    pub fn shown(&self, now: i64) -> Vec<&HistoryRow> {
        let mut rows: Vec<&HistoryRow> = self
            .rows
            .iter()
            .filter(|row| detail::accepts(row, self.range, self.kind, now))
            .collect();
        rows.sort_by(|left, right| right.at.cmp(&left.at).then(right.label.cmp(&left.label)));
        rows
    }
}

/// Applies `message` and returns a reload when the range changed.
#[must_use]
pub fn apply(page: &mut HistoryPage, message: HistMsg) -> Vec<DaemonCall> {
    match message {
        HistMsg::Range(range) => {
            page.range = range;
            vec![reload(page)]
        }
        HistMsg::Kind(kind) => {
            page.kind = kind;
            Vec::new()
        }
        HistMsg::Select(at) => {
            page.selected = Some(at);
            Vec::new()
        }
        HistMsg::Loaded(load) => {
            page.rows = load.rows;
            page.source = Some(load.source);
            page.error = None;
            if page
                .selected
                .is_some_and(|at| !page.rows.iter().any(|row| row.at == at))
            {
                page.selected = None;
            }
            Vec::new()
        }
        HistMsg::Failed(text) => {
            page.error = Some(text);
            Vec::new()
        }
    }
}

fn reload(page: &HistoryPage) -> DaemonCall {
    DaemonCall::LoadHistory {
        since_seconds: page.since_seconds(),
    }
}

/// Unix seconds for the filter. Tests pass their own clock.
#[must_use]
pub fn now_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| {
            i64::try_from(duration.as_secs()).unwrap_or(i64::MAX)
        })
}
