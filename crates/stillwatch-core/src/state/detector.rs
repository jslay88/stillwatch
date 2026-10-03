use crate::event::CaptureFrame;
use crate::stats::DetectionStats;

/// Turns capture ticks into stale verdicts.
///
/// The [`StateMachine`](super::StateMachine) owns one and feeds it every
/// capture it asked for. The block persistence detector implements it for
/// real; tests use [`ScriptedDetector`](crate::mocks::ScriptedDetector).
pub trait StaleDetector: Send {
    /// Feed one capture tick. `playing` is the list of currently playing MPRIS player names.
    fn observe(&mut self, frames: &[CaptureFrame], playing: &[String]) -> DetectionStats;
    /// Forget all per-block history (snooze expiry while idle, backend rebuild).
    fn reset(&mut self);
    /// The snooze ceiling over the counters as of the last `observe`: blocks
    /// unchanged for `safety.ceiling_minutes` against
    /// `safety.ceiling_stale_percent`. `None` when the ceiling is disabled.
    fn ceiling(&self) -> Option<DetectionStats>;
}
