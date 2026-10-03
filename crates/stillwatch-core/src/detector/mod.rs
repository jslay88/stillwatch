//! The per-block persistence detector.
//!
//! For every monitored output the detector keeps each block's previous mean
//! luma and how many consecutive captures it stayed within
//! `luma_delta_threshold`. Each capture it classifies every block as
//! changed, persistent, dark, or ignored, and decides whether the screen is
//! stale. Only those states and percentages leave the detector; luma values
//! never do.
//!
//! The first capture after a reset is a baseline: a constant image becomes
//! persistent on the `persist_checks`-th unchanged capture after it, which is
//! `persist_checks * check_interval_seconds` after the baseline.
//!
//! Ignore regions are mapped to blocks using the output sizes from
//! [`BlockDetector::set_outputs`]; see the `ignore` module for the exact rule.
//! Until an output's size is known, its regions ignore nothing.

mod aggregate;
mod ceiling;
mod ignore;
mod threshold;
mod tracker;

use std::collections::{BTreeMap, HashMap};

use crate::config::{Config, SafetyConfig, StaleConfig};
use crate::luma::OutputInfo;
use crate::stats::{BlockState, DetectionStats};

use tracker::{BlockRules, OutputTracker};

/// One output's mean block luma from a capture, row-major over `block_grid`.
///
/// This is detector input only. It holds luma values, so it must never be
/// logged, stored, or sent anywhere.
#[derive(Debug, Clone, Copy)]
pub struct BlockMeans<'a> {
    /// Connector name, for example `HDMI-A-1`.
    pub output: &'a str,
    /// `cols * rows` mean luma values (0-255), row-major.
    pub means: &'a [f32],
}

/// Detects stale (burn-in risk) content from per-block persistence.
#[derive(Debug, Clone)]
pub struct BlockDetector {
    stale: StaleConfig,
    safety: SafetyConfig,
    rules: BlockRules,
    outputs: Vec<OutputInfo>,
    masks: HashMap<String, Vec<bool>>,
    trackers: BTreeMap<String, OutputTracker>,
}

impl BlockDetector {
    /// Creates a detector from the `[stale]` and `[safety]` sections.
    #[must_use]
    pub fn new(config: &Config) -> Self {
        Self {
            stale: config.stale.clone(),
            safety: config.safety.clone(),
            rules: BlockRules::from_config(&config.stale),
            outputs: Vec::new(),
            masks: HashMap::new(),
            trackers: BTreeMap::new(),
        }
    }

    /// Applies a reloaded config.
    ///
    /// Thresholds, deltas, and ignore regions apply from the next capture
    /// without losing history. Changing `block_grid` or `monitored_outputs`
    /// resets every block counter.
    pub fn apply_config(&mut self, config: &Config) {
        let resets = self.stale.block_grid != config.stale.block_grid
            || sorted(&self.stale.monitored_outputs) != sorted(&config.stale.monitored_outputs);
        self.stale = config.stale.clone();
        self.safety = config.safety.clone();
        self.rules = BlockRules::from_config(&config.stale);
        if resets {
            self.reset();
        }
        self.rebuild_masks();
    }

    /// Records the connected outputs (from `Event::OutputsChanged`).
    ///
    /// Their sizes map ignore regions to blocks. Block state for outputs that
    /// are no longer connected is dropped.
    pub fn set_outputs(&mut self, outputs: &[OutputInfo]) {
        self.outputs = outputs.to_vec();
        self.trackers
            .retain(|name, _| outputs.iter().any(|output| &output.name == name));
        self.rebuild_masks();
    }

    /// Forgets the block state of one output (hotplug removal).
    pub fn drop_output(&mut self, output: &str) {
        self.trackers.remove(output);
    }

    /// Forgets all per-block history (snooze expiry while idle, backend rebuild).
    pub fn reset(&mut self) {
        self.trackers.clear();
    }

    /// Whether `output` is monitored (`monitored_outputs` empty means all).
    #[must_use]
    pub fn monitors(&self, output: &str) -> bool {
        self.stale.monitored_outputs.is_empty()
            || self
                .stale
                .monitored_outputs
                .iter()
                .any(|name| name == output)
    }

    /// The block grid as `[cols, rows]`.
    #[must_use]
    pub const fn grid(&self) -> [u32; 2] {
        self.stale.block_grid
    }

    /// Feeds one capture tick of block means. `playing` is the list of
    /// currently playing MPRIS player names.
    ///
    /// Unmonitored outputs are skipped. An output whose means don't match
    /// `block_grid` has its history reset and is left out of this tick.
    pub fn observe_means(
        &mut self,
        outputs: &[BlockMeans<'_>],
        playing: &[String],
    ) -> DetectionStats {
        let blocks = self.block_count();
        let mut observed = Vec::with_capacity(outputs.len());
        for frame in outputs {
            if !self.monitors(frame.output) {
                continue;
            }
            if frame.means.len() != blocks {
                self.trackers.remove(frame.output);
                continue;
            }
            let mask = self.masks.get(frame.output).map_or(&[][..], Vec::as_slice);
            self.trackers
                .entry(frame.output.to_owned())
                .or_default()
                .update(frame.means, mask, &self.rules);
            observed.push(frame.output);
        }
        let states = observed
            .into_iter()
            .filter_map(|name| Some((name, self.blocks(name)?)));
        aggregate::summarize(
            states,
            threshold::select(&self.stale, playing),
            self.stale.require,
        )
    }

    /// The snooze ceiling over the last capture of every tracked output.
    ///
    /// A counted block is persistent here once it has been unchanged for
    /// `ceiling_minutes` worth of captures, and the threshold is
    /// `ceiling_stale_percent` ([`ThresholdReason::Ceiling`]). `require`
    /// applies across outputs as usual. `None` when the ceiling is disabled.
    ///
    /// [`ThresholdReason::Ceiling`]: crate::stats::ThresholdReason::Ceiling
    #[must_use]
    pub fn ceiling(&self) -> Option<DetectionStats> {
        if !self.safety.ceiling_enabled {
            return None;
        }
        let checks = ceiling::ceiling_checks(&self.stale, &self.safety);
        let states: Vec<(&str, Vec<BlockState>)> = self
            .trackers
            .iter()
            .map(|(name, tracker)| (name.as_str(), tracker.states_after(checks)))
            .collect();
        Some(aggregate::summarize(
            states
                .iter()
                .map(|(name, states)| (*name, states.as_slice())),
            ceiling::ceiling_threshold(&self.safety),
            self.stale.require,
        ))
    }

    /// Row-major block states from `output`'s last capture, for probe and
    /// the calibration heatmap. `None` if the output hasn't been captured
    /// since the last reset.
    #[must_use]
    pub fn blocks(&self, output: &str) -> Option<&[BlockState]> {
        self.trackers
            .get(output)
            .map(OutputTracker::states)
            .filter(|states| !states.is_empty())
    }

    fn block_count(&self) -> usize {
        let [cols, rows] = self.stale.block_grid;
        cols as usize * rows as usize
    }

    fn rebuild_masks(&mut self) {
        self.masks = self
            .outputs
            .iter()
            .map(|output| {
                let mask = ignore::ignore_mask(&self.stale.ignore_regions, output, self.grid());
                (output.name.clone(), mask)
            })
            .filter(|(_, mask)| !mask.is_empty())
            .collect();
    }
}

fn sorted(names: &[String]) -> Vec<&str> {
    let mut names: Vec<&str> = names.iter().map(String::as_str).collect();
    names.sort_unstable();
    names
}

#[cfg(test)]
mod proptests;
#[cfg(test)]
mod tests;
