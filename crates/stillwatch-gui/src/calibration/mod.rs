//! Calibration heatmap: probe lifetime, block colors, and ignore regions.
//!
//! A probe sample contributes per-block states, percentages, and each output's
//! pixel size (so a drawn rectangle can be stored as an ignore region). Luma
//! values and frame pixels are never kept.

mod draw;
mod edit;
mod heat;
mod live;
mod view;

#[cfg(test)]
mod tests;

pub(crate) use edit::handle;
pub(crate) use heat::view_of;
pub(crate) use live::{interval_ms, next_call};
pub(crate) use view::page;

use stillwatch_core::stats::{BlockState, ThresholdReason};

/// How often the calibration page asks the daemon to sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProbePace {
    /// One sample a second.
    OneSecond,
    /// One sample every 5 seconds.
    #[default]
    FiveSeconds,
    /// `stale.check_interval_seconds` from the settings form.
    CheckInterval,
}

impl std::fmt::Display for ProbePace {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.label())
    }
}

impl ProbePace {
    /// Choices in the order the page lists them.
    pub const ALL: [Self; 3] = [Self::OneSecond, Self::FiveSeconds, Self::CheckInterval];

    /// Label on the interval control.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::OneSecond => "1 s",
            Self::FiveSeconds => "5 s",
            Self::CheckInterval => "Check interval",
        }
    }
}

/// One output's grid and the numbers its summary line shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputHeat {
    /// Connector name.
    pub name: String,
    /// Block columns.
    pub columns: u16,
    /// Block rows.
    pub rows: u16,
    /// Output width in pixels. Zero when the sample didn't include a size.
    pub width: u32,
    /// Output height in pixels. Zero when the sample didn't include a size.
    pub height: u32,
    /// Row-major block states.
    pub cells: Vec<BlockState>,
    /// Counted blocks that are persistent.
    pub persistent: u32,
    /// Dark blocks.
    pub dark: u32,
    /// Blocks that are neither dark nor ignored.
    pub counted: u32,
    /// Every block in the grid.
    pub total: u32,
    /// Persistent blocks as a percent of counted blocks, already rounded.
    pub persistent_percent: String,
    /// Dark blocks as a percent of all blocks, already rounded.
    pub dark_percent: String,
    /// Whether this output met the threshold.
    pub stale: bool,
}

impl OutputHeat {
    /// The per-output line: persistent %, dark %, counted blocks, threshold, verdict.
    #[must_use]
    pub fn summary(&self, threshold: &str) -> String {
        let verdict = if self.stale { "STALE" } else { "not stale" };
        format!(
            "{}: persistent {} (dark {}, counted {}/{}), threshold {threshold} -> {verdict}",
            self.name, self.persistent_percent, self.dark_percent, self.counted, self.total,
        )
    }
}

/// The latest probe sample, reduced to states and percentages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeView {
    /// Threshold percent that was applied.
    pub threshold_percent: u8,
    /// Why that threshold was used.
    pub threshold_reason: ThresholdReason,
    /// Whether the screen as a whole was stale.
    pub stale: bool,
    /// One heatmap per monitored output in the sample.
    pub outputs: Vec<OutputHeat>,
}

impl ProbeView {
    /// `70% normal`, `90% media`, or `98% ceiling`.
    #[must_use]
    pub fn threshold_label(&self) -> String {
        let reason = match self.threshold_reason {
            ThresholdReason::Normal => "normal",
            ThresholdReason::Media => "media",
            ThresholdReason::Ceiling => "ceiling",
        };
        format!("{}% {reason}", self.threshold_percent)
    }
}

/// A drag on one output's grid, in block coordinates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drag {
    /// Output the pointer went down on.
    pub output: String,
    /// Block column where the drag started.
    pub start_column: u16,
    /// Block row where the drag started.
    pub start_row: u16,
    /// Block column the pointer is on now.
    pub column: u16,
    /// Block row the pointer is on now.
    pub row: u16,
}

/// Calibration page state. The shell owns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Calibration {
    /// Selected probe interval.
    pub pace: ProbePace,
    /// `StartProbe` has been sent and `StopProbe` has not.
    pub(crate) running: bool,
    /// Interval of the `StartProbe` currently in effect, in milliseconds.
    pub(crate) sent_ms: u32,
    /// Latest sample, when one has arrived while the page was open.
    pub view: Option<ProbeView>,
    /// Drag in progress.
    pub drag: Option<Drag>,
    /// Region the next drag replaces. `None` appends.
    pub editing: Option<usize>,
    /// Why the last drag wasn't saved onto the form.
    pub place_error: Option<String>,
}

impl Default for Calibration {
    fn default() -> Self {
        Self {
            pace: ProbePace::FiveSeconds,
            running: false,
            sent_ms: 0,
            view: None,
            drag: None,
            editing: None,
            place_error: None,
        }
    }
}

impl Calibration {
    /// Shown when the daemon is up, status has arrived, and no capture backend is running.
    #[must_use]
    pub const fn idle_only_notice(
        daemon_up: bool,
        capture_known: bool,
        capture_backend: Option<&str>,
    ) -> Option<&'static str> {
        if daemon_up && capture_known && capture_backend.is_none() {
            Some(
                "No capture backend is available. Stillwatch is in input-idle-only mode, so there is no heatmap.",
            )
        } else {
            None
        }
    }
}

/// What the calibration page asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalMsg {
    /// Change how often samples are requested.
    Pace(ProbePace),
    /// A tuning slider moved. The value is written through the settings form.
    Slider {
        /// Schema key.
        key: String,
        /// New whole number.
        value: u32,
    },
    /// Pointer went down on `output`'s grid.
    BeginDrag {
        /// Connector name.
        output: String,
        /// Block column.
        column: u16,
        /// Block row.
        row: u16,
    },
    /// Pointer moved while a drag is active.
    MoveDrag {
        /// Block column.
        column: u16,
        /// Block row.
        row: u16,
    },
    /// Pointer released. Converts the drag into an ignore region.
    FinishDrag,
    /// The next drag replaces this region.
    Edit(usize),
    /// Remove this region from the form.
    Delete(usize),
}
