//! The `ProbeSample` signal payload: per-block states and percentages only.

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use stillwatch_core::stats::{BlockCounts, BlockState, OutputStats, Threshold};

use crate::error::IpcError;

/// The shortest `StartProbe` interval the daemon accepts, in milliseconds.
pub const MIN_PROBE_INTERVAL_MS: u32 = 100;

/// One output's probe result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProbeOutput {
    /// Percentages and per-output verdict (flattened into this object).
    #[serde(flatten)]
    pub stats: OutputStats,
    /// Block grid columns.
    pub columns: u16,
    /// Block grid rows.
    pub rows: u16,
    /// Row-major block states, `columns * rows` long.
    pub blocks: Vec<BlockState>,
}

impl ProbeOutput {
    /// Builds a probe result from block states, computing the percentages and
    /// the stale verdict against `threshold_percent`.
    ///
    /// # Errors
    ///
    /// Returns [`IpcError::ProbeGrid`] if `blocks` isn't `columns * rows` long.
    pub fn from_blocks(
        output: impl Into<String>,
        columns: u16,
        rows: u16,
        blocks: Vec<BlockState>,
        threshold_percent: u8,
    ) -> Result<Self, IpcError> {
        let expected = usize::from(columns) * usize::from(rows);
        if blocks.len() != expected {
            return Err(IpcError::ProbeGrid {
                columns,
                rows,
                expected,
                actual: blocks.len(),
            });
        }
        let counts = BlockCounts::from_states(&blocks);
        Ok(Self {
            stats: OutputStats::from_counts(output, counts, threshold_percent),
            columns,
            rows,
            blocks,
        })
    }

    /// The state of the block at `(column, row)`, or `None` when out of range.
    #[must_use]
    pub fn block(&self, column: u16, row: u16) -> Option<BlockState> {
        if column >= self.columns || row >= self.rows {
            return None;
        }
        let index = usize::from(row) * usize::from(self.columns) + usize::from(column);
        self.blocks.get(index).copied()
    }
}

/// A calibration sample emitted while a probe is running.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProbeSample {
    /// When the capture was taken.
    pub at: Timestamp,
    /// The threshold applied.
    pub threshold: Threshold,
    /// Whether the screen as a whole was stale.
    pub stale: bool,
    /// One entry per monitored output.
    pub outputs: Vec<ProbeOutput>,
}

#[cfg(test)]
mod tests {
    use stillwatch_core::stats::ThresholdReason;

    use super::*;
    use crate::json::{from_json, to_json};

    fn two_by_two() -> ProbeOutput {
        let blocks = vec![
            BlockState::Persistent,
            BlockState::Changed,
            BlockState::Dark,
            BlockState::Ignored,
        ];
        ProbeOutput::from_blocks("DP-1", 2, 2, blocks, 50).unwrap()
    }

    #[test]
    fn from_blocks_computes_stats() {
        let output = two_by_two();
        assert_eq!(output.stats.output, "DP-1");
        assert!((output.stats.persistent_percent - 50.0).abs() < f64::EPSILON);
        assert!((output.stats.counted_percent - 50.0).abs() < f64::EPSILON);
        assert!(output.stats.stale);
    }

    #[test]
    fn block_lookup_is_row_major() {
        let output = two_by_two();
        assert_eq!(output.block(1, 0), Some(BlockState::Changed));
        assert_eq!(output.block(0, 1), Some(BlockState::Dark));
        assert_eq!(output.block(2, 0), None);
        assert_eq!(output.block(0, 2), None);
    }

    #[test]
    fn mismatched_grid_is_rejected() {
        let err = ProbeOutput::from_blocks("DP-1", 3, 2, vec![BlockState::Dark], 70).unwrap_err();
        assert_eq!(err.to_string(), "probe grid 3x2 needs 6 blocks, got 1");
    }

    #[test]
    fn sample_round_trips_without_luma() {
        let sample = ProbeSample {
            at: Timestamp::from_second(1_790_000_000).unwrap(),
            threshold: Threshold::new(70, ThresholdReason::Normal),
            stale: true,
            outputs: vec![two_by_two()],
        };
        let json = to_json(&sample).unwrap();
        assert!(json.contains(r#""output":"DP-1""#));
        assert!(json.contains(r#""blocks":["persistent","changed","dark","ignored"]"#));
        assert!(!json.contains("luma"));
        assert_eq!(from_json::<ProbeSample>(&json).unwrap(), sample);
    }
}
