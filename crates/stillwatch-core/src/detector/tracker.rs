//! Per-output block history: previous mean luma and `unchanged_count`.

use crate::config::StaleConfig;
use crate::stats::BlockState;

/// The `[stale]` settings that classify a single block.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct BlockRules {
    luma_delta: f32,
    dark_below: f32,
    persist_checks: u32,
}

impl BlockRules {
    pub(crate) fn from_config(stale: &StaleConfig) -> Self {
        Self {
            luma_delta: luma(stale.luma_delta_threshold),
            dark_below: luma(stale.ignore_dark_below),
            persist_checks: stale.persist_checks,
        }
    }

    fn is_dark(&self, mean: f32) -> bool {
        mean < self.dark_below
    }
}

fn luma(value: u32) -> f32 {
    f32::from(u8::try_from(value).unwrap_or(u8::MAX))
}

/// History for every block of one output.
///
/// `unchanged_count` is updated for every block, including dark and ignored
/// ones, so it always means "consecutive captures within
/// `luma_delta_threshold`". Dark and ignored only change how a block is
/// classified, never what the counter holds.
#[derive(Debug, Clone, Default)]
pub(crate) struct OutputTracker {
    previous: Vec<f32>,
    unchanged: Vec<u32>,
    states: Vec<BlockState>,
}

impl OutputTracker {
    /// Feeds one capture's block means (row-major) and classifies each block.
    ///
    /// `ignored` is the ignore-region mask for this output; an empty mask
    /// ignores nothing. The first capture after creation is a baseline, so
    /// every non-ignored block starts as [`BlockState::Changed`].
    pub(crate) fn update(&mut self, means: &[f32], ignored: &[bool], rules: &BlockRules) {
        if self.unchanged.len() != means.len() {
            *self = Self::default();
            self.unchanged.resize(means.len(), 0);
        }
        self.states.clear();
        for (index, (&current, unchanged)) in means.iter().zip(&mut self.unchanged).enumerate() {
            let previous = self.previous.get(index).copied();
            *unchanged = match previous {
                Some(previous) if (current - previous).abs() <= rules.luma_delta => {
                    unchanged.saturating_add(1)
                }
                _ => 0,
            };
            let dark =
                previous.is_some_and(|previous| rules.is_dark(previous) && rules.is_dark(current));
            let state = if ignored.get(index).copied().unwrap_or(false) {
                BlockState::Ignored
            } else if dark {
                BlockState::Dark
            } else {
                counted_state(*unchanged, rules.persist_checks)
            };
            self.states.push(state);
        }
        self.previous.clear();
        self.previous.extend_from_slice(means);
    }

    /// Block states from the last capture, row-major. Empty before the first.
    pub(crate) fn states(&self) -> &[BlockState] {
        &self.states
    }

    /// The last capture's states with counted blocks re-judged against
    /// `checks` instead of `persist_checks`.
    pub(crate) fn states_after(&self, checks: u32) -> Vec<BlockState> {
        self.states
            .iter()
            .zip(&self.unchanged)
            .map(|(&state, &unchanged)| match state {
                BlockState::Changed | BlockState::Persistent => counted_state(unchanged, checks),
                other => other,
            })
            .collect()
    }
}

fn counted_state(unchanged: u32, checks: u32) -> BlockState {
    if unchanged >= checks {
        BlockState::Persistent
    } else {
        BlockState::Changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(luma_delta: u32, dark_below: u32, persist_checks: u32) -> BlockRules {
        BlockRules::from_config(&StaleConfig {
            luma_delta_threshold: luma_delta,
            ignore_dark_below: dark_below,
            persist_checks,
            ..StaleConfig::default()
        })
    }

    #[test]
    fn first_capture_is_a_baseline() {
        let mut tracker = OutputTracker::default();
        tracker.update(&[100.0, 0.0], &[], &rules(6, 16, 1));
        assert_eq!(tracker.states(), &[BlockState::Changed; 2]);
    }

    #[test]
    fn delta_at_the_threshold_counts_as_unchanged() {
        let rules = rules(6, 0, 1);
        let mut tracker = OutputTracker::default();
        tracker.update(&[100.0, 100.0], &[], &rules);
        tracker.update(&[106.0, 106.5], &[], &rules);
        assert_eq!(
            tracker.states(),
            &[BlockState::Persistent, BlockState::Changed]
        );
    }

    #[test]
    fn dark_needs_both_captures_below_the_cutoff() {
        let rules = rules(6, 16, 1);
        let mut tracker = OutputTracker::default();
        tracker.update(&[10.0, 10.0, 20.0], &[], &rules);
        tracker.update(&[12.0, 16.0, 14.0], &[], &rules);
        assert_eq!(
            tracker.states(),
            &[
                BlockState::Dark,
                BlockState::Persistent,
                BlockState::Persistent
            ]
        );
    }

    #[test]
    fn zero_dark_cutoff_disables_dark_blocks() {
        let rules = rules(6, 0, 1);
        let mut tracker = OutputTracker::default();
        tracker.update(&[0.0], &[], &rules);
        tracker.update(&[0.0], &[], &rules);
        assert_eq!(tracker.states(), &[BlockState::Persistent]);
    }

    #[test]
    fn counter_keeps_running_while_a_block_is_dark() {
        let rules = rules(6, 16, 3);
        let mut tracker = OutputTracker::default();
        for mean in [10.0, 12.0, 14.0] {
            tracker.update(&[mean], &[], &rules);
        }
        assert_eq!(tracker.states(), &[BlockState::Dark]);
        tracker.update(&[18.0], &[], &rules);
        assert_eq!(tracker.states(), &[BlockState::Persistent]);
    }

    #[test]
    fn ignored_wins_over_every_other_state() {
        let rules = rules(6, 16, 1);
        let mut tracker = OutputTracker::default();
        tracker.update(&[0.0, 200.0], &[true, true], &rules);
        tracker.update(&[0.0, 200.0], &[true, true], &rules);
        assert_eq!(tracker.states(), &[BlockState::Ignored; 2]);
        assert_eq!(tracker.states_after(1), vec![BlockState::Ignored; 2]);
    }

    #[test]
    fn states_after_rejudges_counted_blocks() {
        let rules = rules(6, 16, 1);
        let mut tracker = OutputTracker::default();
        for _ in 0..3 {
            tracker.update(&[100.0, 0.0], &[], &rules);
        }
        assert_eq!(tracker.states_after(2)[0], BlockState::Persistent);
        assert_eq!(tracker.states_after(3)[0], BlockState::Changed);
        assert_eq!(tracker.states_after(3)[1], BlockState::Dark);
    }

    #[test]
    fn grid_size_change_starts_over() {
        let rules = rules(6, 0, 1);
        let mut tracker = OutputTracker::default();
        tracker.update(&[1.0, 1.0], &[], &rules);
        tracker.update(&[1.0, 1.0, 1.0], &[], &rules);
        assert_eq!(tracker.states(), &[BlockState::Changed; 3]);
    }
}
