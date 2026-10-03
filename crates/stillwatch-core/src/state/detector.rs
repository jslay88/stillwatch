use crate::backend::MediaPlayer;
use crate::config::Config;
use crate::event::CaptureFrame;
use crate::luma::OutputInfo;
use crate::stats::DetectionStats;

/// Turns capture ticks into stale verdicts.
///
/// The [`StateMachine`](super::StateMachine) owns one and feeds it every
/// capture it asked for, every reloaded config, and every output change.
/// [`BlockDetector`](crate::detector::BlockDetector) implements it for real;
/// tests use [`ScriptedDetector`](crate::mocks::ScriptedDetector).
pub trait StaleDetector: Send {
    /// Feed one capture tick. `playing` is every MPRIS player currently reporting `Playing`.
    fn observe(&mut self, frames: &[CaptureFrame], playing: &[MediaPlayer]) -> DetectionStats;
    /// Forget all per-block history (snooze expiry while idle, backend rebuild).
    fn reset(&mut self);
    /// The snooze ceiling over the counters as of the last `observe`: blocks
    /// unchanged for `safety.ceiling_minutes` against
    /// `safety.ceiling_stale_percent`. `None` when the ceiling is disabled.
    fn ceiling(&self) -> Option<DetectionStats>;
    /// Take a reloaded config. Thresholds, deltas, and ignore regions apply
    /// from the next capture and keep the block history. Changing a key that
    /// resets detection (`capture.backend`, `stale.block_grid`,
    /// `stale.monitored_outputs`) forgets all of it.
    fn apply_config(&mut self, config: &Config);
    /// Take the full list of connected outputs (`Event::OutputsChanged`).
    /// Their sizes place ignore regions; outputs missing from the list lose
    /// their block history.
    fn set_outputs(&mut self, outputs: &[OutputInfo]);
}
