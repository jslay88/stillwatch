//! Detection results that are safe to share: block states and percentages.
//!
//! These types are what the detector reports, what the decision history
//! records, and what probe samples and status payloads carry. None of them
//! hold luma values or pixels.

use serde::{Deserialize, Serialize};

/// The state of one block of the detection grid after a capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockState {
    /// Counted, but not unchanged for `persist_checks` captures (yet).
    Changed,
    /// Counted, and unchanged for at least `persist_checks` captures.
    Persistent,
    /// Below `ignore_dark_below` in both the previous and current capture.
    Dark,
    /// Inside an ignore region.
    Ignored,
}

/// Why a particular stale threshold was used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThresholdReason {
    /// `stale_percent`: no non-ignored media player was playing.
    Normal,
    /// `media_stale_percent`: a non-ignored media player was playing.
    Media,
    /// `ceiling_stale_percent`: the snooze/pause ceiling check.
    Ceiling,
}

/// The stale threshold applied to a decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Threshold {
    /// Percent of counted blocks that had to be persistent (0-100).
    pub percent: u8,
    /// Which setting the percentage came from.
    pub reason: ThresholdReason,
}

impl Threshold {
    /// Creates a threshold.
    #[must_use]
    pub const fn new(percent: u8, reason: ThresholdReason) -> Self {
        Self { percent, reason }
    }
}

/// Block tallies for one output.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct BlockCounts {
    /// Every block in the grid.
    pub total: u32,
    /// Blocks that are neither dark nor ignored.
    pub counted: u32,
    /// Counted blocks that are persistent.
    pub persistent: u32,
    /// Dark blocks.
    pub dark: u32,
    /// Ignored blocks.
    pub ignored: u32,
}

impl BlockCounts {
    /// Tallies a slice of block states.
    #[must_use]
    pub fn from_states(states: &[BlockState]) -> Self {
        states.iter().fold(Self::default(), |mut counts, state| {
            counts.total = counts.total.saturating_add(1);
            match state {
                BlockState::Changed => counts.counted = counts.counted.saturating_add(1),
                BlockState::Persistent => {
                    counts.counted = counts.counted.saturating_add(1);
                    counts.persistent = counts.persistent.saturating_add(1);
                }
                BlockState::Dark => counts.dark = counts.dark.saturating_add(1),
                BlockState::Ignored => counts.ignored = counts.ignored.saturating_add(1),
            }
            counts
        })
    }
}

/// Detection percentages for one output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputStats {
    /// Connector name, for example `HDMI-A-1`.
    pub output: String,
    /// Persistent blocks as a percent of counted blocks (the stale fraction).
    pub persistent_percent: f64,
    /// Dark blocks as a percent of all blocks.
    pub dark_percent: f64,
    /// Counted blocks as a percent of all blocks.
    pub counted_percent: f64,
    /// Whether this output met the threshold. Always false with no counted blocks.
    pub stale: bool,
}

impl OutputStats {
    /// Computes the percentages from block tallies and decides staleness
    /// against `threshold_percent`.
    #[must_use]
    pub fn from_counts(
        output: impl Into<String>,
        counts: BlockCounts,
        threshold_percent: u8,
    ) -> Self {
        let persistent_percent = percent(counts.persistent, counts.counted);
        Self {
            output: output.into(),
            persistent_percent,
            dark_percent: percent(counts.dark, counts.total),
            counted_percent: percent(counts.counted, counts.total),
            stale: counts.counted > 0 && persistent_percent >= f64::from(threshold_percent),
        }
    }
}

/// The detector's verdict for one capture across all monitored outputs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DetectionStats {
    /// Per-output percentages.
    pub outputs: Vec<OutputStats>,
    /// The threshold that was applied.
    pub threshold: Threshold,
    /// Whether the screen as a whole was stale (per `stale.require`).
    pub stale: bool,
}

fn percent(part: u32, whole: u32) -> f64 {
    if whole == 0 {
        0.0
    } else {
        f64::from(part) * 100.0 / f64::from(whole)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATES: [BlockState; 8] = [
        BlockState::Persistent,
        BlockState::Persistent,
        BlockState::Persistent,
        BlockState::Changed,
        BlockState::Dark,
        BlockState::Dark,
        BlockState::Ignored,
        BlockState::Ignored,
    ];

    #[test]
    fn counts_tally_each_state() {
        let counts = BlockCounts::from_states(&STATES);
        assert_eq!(
            counts,
            BlockCounts {
                total: 8,
                counted: 4,
                persistent: 3,
                dark: 2,
                ignored: 2
            }
        );
    }

    #[test]
    fn stats_use_counted_blocks_as_the_stale_denominator() {
        let stats = OutputStats::from_counts("DP-1", BlockCounts::from_states(&STATES), 75);
        assert_eq!(stats.output, "DP-1");
        assert!((stats.persistent_percent - 75.0).abs() < f64::EPSILON);
        assert!((stats.dark_percent - 25.0).abs() < f64::EPSILON);
        assert!((stats.counted_percent - 50.0).abs() < f64::EPSILON);
        assert!(stats.stale);
        assert!(!OutputStats::from_counts("DP-1", BlockCounts::from_states(&STATES), 76).stale);
    }

    #[test]
    fn all_dark_output_is_never_stale() {
        let counts = BlockCounts::from_states(&[BlockState::Dark; 4]);
        let stats = OutputStats::from_counts("DP-1", counts, 0);
        assert!(!stats.stale);
        assert!(stats.persistent_percent.abs() < f64::EPSILON);
        assert!((stats.dark_percent - 100.0).abs() < f64::EPSILON);
        let empty = OutputStats::from_counts("DP-1", BlockCounts::default(), 0);
        assert!(!empty.stale);
        assert!(empty.dark_percent.abs() < f64::EPSILON);
    }

    #[test]
    fn detection_stats_round_trip_through_json() {
        let stats = DetectionStats {
            outputs: vec![OutputStats::from_counts(
                "HDMI-A-1",
                BlockCounts::from_states(&STATES),
                70,
            )],
            threshold: Threshold::new(70, ThresholdReason::Media),
            stale: true,
        };
        let json = serde_json::to_string(&stats).unwrap();
        assert!(json.contains(r#""reason":"media""#));
        assert_eq!(
            serde_json::from_str::<DetectionStats>(&json).unwrap(),
            stats
        );
        let encoded = serde_json::to_string(&STATES).unwrap();
        assert!(encoded.starts_with(r#"["persistent","persistent","persistent","changed","dark""#));
    }
}
