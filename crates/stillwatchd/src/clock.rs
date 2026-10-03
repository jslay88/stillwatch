//! A [`Clock`] on tokio's time.

use std::time::Instant;

use jiff::Timestamp;
use stillwatch_core::time::{Clock, SystemClock};

/// Monotonic time from tokio's clock, wall time from the system.
///
/// The same as [`SystemClock`] in a normal runtime. Under tokio's paused
/// test time it follows `tokio::time::advance` and auto-advance, so the
/// timers a loop sleeps on and the `now` it hands to pure logic agree.
#[derive(Debug, Clone, Copy, Default)]
pub struct TokioClock;

impl Clock for TokioClock {
    fn now(&self) -> Instant {
        tokio::time::Instant::now().into_std()
    }

    fn wall_now(&self) -> Timestamp {
        SystemClock.wall_now()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[tokio::test(start_paused = true)]
    async fn follows_paused_tokio_time() {
        let start = TokioClock.now();
        tokio::time::advance(Duration::from_mins(10)).await;
        assert_eq!(TokioClock.now() - start, Duration::from_mins(10));
        assert!(TokioClock.wall_now() > Timestamp::UNIX_EPOCH);
    }
}
