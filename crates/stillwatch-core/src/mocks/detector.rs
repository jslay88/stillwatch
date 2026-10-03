use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::backend::MediaPlayer;
use crate::config::Config;
use crate::event::CaptureFrame;
use crate::luma::OutputInfo;
use crate::state::StaleDetector;
use crate::stats::{BlockCounts, DetectionStats, OutputStats, Threshold, ThresholdReason};
use crate::sync::lock;

/// What one `observe` call received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observation {
    /// Outputs of the frames passed in, in order.
    pub outputs: Vec<String>,
    /// The `playing` list passed in.
    pub playing: Vec<MediaPlayer>,
}

/// A [`StaleDetector`] that returns queued verdicts and records its calls.
///
/// Clones share state, so a test keeps one clone to script and inspect while
/// the machine owns another. With nothing queued, `observe` reports a fresh
/// (not stale) screen. `ceiling` returns whatever
/// [`set_ceiling`](Self::set_ceiling) last set, `None` until then.
#[derive(Debug, Clone, Default)]
pub struct ScriptedDetector {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Debug, Default)]
struct Inner {
    verdicts: VecDeque<DetectionStats>,
    observations: Vec<Observation>,
    resets: usize,
    ceiling: Option<DetectionStats>,
    ceiling_queries: usize,
    configs: Vec<Config>,
    outputs: Vec<Vec<OutputInfo>>,
}

impl ScriptedDetector {
    /// A detector with nothing queued.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues the verdict for the next `observe`.
    pub fn push(&self, stats: DetectionStats) {
        lock(&self.inner).verdicts.push_back(stats);
    }

    /// Queues a stale or fresh verdict built by [`verdict`](Self::verdict).
    pub fn push_verdict(&self, stale: bool) {
        self.push(Self::verdict(stale));
    }

    /// Sets what every later `ceiling` call returns.
    pub fn set_ceiling(&self, stats: Option<DetectionStats>) {
        lock(&self.inner).ceiling = stats;
    }

    /// A one-output verdict on `HDMI-A-1`: every block persistent when
    /// `stale`, none otherwise, against the default 70% threshold.
    #[must_use]
    pub fn verdict(stale: bool) -> DetectionStats {
        Self::stats(stale, Threshold::new(70, ThresholdReason::Normal))
    }

    /// Like [`verdict`](Self::verdict), against the default 98% ceiling.
    #[must_use]
    pub fn ceiling_verdict(stale: bool) -> DetectionStats {
        Self::stats(stale, Threshold::new(98, ThresholdReason::Ceiling))
    }

    fn stats(stale: bool, threshold: Threshold) -> DetectionStats {
        let counts = BlockCounts {
            total: 256,
            counted: 256,
            persistent: if stale { 256 } else { 0 },
            dark: 0,
            ignored: 0,
        };
        DetectionStats {
            outputs: vec![OutputStats::from_counts(
                "HDMI-A-1",
                counts,
                threshold.percent,
            )],
            threshold,
            stale,
        }
    }

    /// Every `observe` call, oldest first.
    #[must_use]
    pub fn observations(&self) -> Vec<Observation> {
        lock(&self.inner).observations.clone()
    }

    /// How many times `reset` was called.
    #[must_use]
    pub fn resets(&self) -> usize {
        lock(&self.inner).resets
    }

    /// How many times `ceiling` was called.
    #[must_use]
    pub fn ceiling_queries(&self) -> usize {
        lock(&self.inner).ceiling_queries
    }

    /// Every config passed to `apply_config`, oldest first.
    #[must_use]
    pub fn configs(&self) -> Vec<Config> {
        lock(&self.inner).configs.clone()
    }

    /// Every output list passed to `set_outputs`, oldest first.
    #[must_use]
    pub fn output_updates(&self) -> Vec<Vec<OutputInfo>> {
        lock(&self.inner).outputs.clone()
    }
}

impl StaleDetector for ScriptedDetector {
    fn observe(&mut self, frames: &[CaptureFrame], playing: &[MediaPlayer]) -> DetectionStats {
        let mut inner = lock(&self.inner);
        inner.observations.push(Observation {
            outputs: frames.iter().map(|frame| frame.output.clone()).collect(),
            playing: playing.to_vec(),
        });
        inner
            .verdicts
            .pop_front()
            .unwrap_or_else(|| Self::verdict(false))
    }

    fn reset(&mut self) {
        lock(&self.inner).resets += 1;
    }

    fn ceiling(&self) -> Option<DetectionStats> {
        let mut inner = lock(&self.inner);
        inner.ceiling_queries += 1;
        inner.ceiling.clone()
    }

    fn apply_config(&mut self, config: &Config) {
        lock(&self.inner).configs.push(config.clone());
    }

    fn set_outputs(&mut self, outputs: &[OutputInfo]) {
        lock(&self.inner).outputs.push(outputs.to_vec());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::luma::LumaGrid;

    #[test]
    fn returns_queued_verdicts_then_fresh_and_records_calls() {
        let handle = ScriptedDetector::new();
        let mut detector = handle.clone();
        handle.push_verdict(true);
        let frame = CaptureFrame {
            output: "DP-1".into(),
            grid: LumaGrid::filled(2, 2, 0).unwrap(),
        };
        assert!(detector.observe(&[frame], &["mpv".into()]).stale);
        assert!(!detector.observe(&[], &[]).stale);
        detector.reset();
        assert_eq!(handle.resets(), 1);
        assert_eq!(
            handle.observations(),
            vec![
                Observation {
                    outputs: vec!["DP-1".into()],
                    playing: vec!["mpv".into()],
                },
                Observation {
                    outputs: vec![],
                    playing: vec![],
                },
            ]
        );
    }

    #[test]
    fn ceiling_is_sticky_and_counted() {
        let handle = ScriptedDetector::new();
        let detector = handle.clone();
        assert_eq!(detector.ceiling(), None);
        handle.set_ceiling(Some(ScriptedDetector::ceiling_verdict(true)));
        for _ in 0..2 {
            let stats = detector.ceiling().unwrap();
            assert!(stats.stale);
            assert_eq!(stats.threshold.reason, ThresholdReason::Ceiling);
        }
        assert_eq!(handle.ceiling_queries(), 3);
    }

    #[test]
    fn records_configs_and_output_updates() {
        let handle = ScriptedDetector::new();
        let mut detector = handle.clone();
        let mut config = Config::default();
        config.stale.stale_percent = 50;
        detector.apply_config(&config);
        detector.set_outputs(&[OutputInfo::new("DP-1", 3840, 2160)]);
        detector.set_outputs(&[]);
        assert_eq!(handle.configs(), vec![config]);
        assert_eq!(
            handle.output_updates(),
            vec![vec![OutputInfo::new("DP-1", 3840, 2160)], vec![]]
        );
    }
}
