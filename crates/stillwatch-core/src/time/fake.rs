use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use jiff::Timestamp;

use super::Clock;
use crate::sync::lock;

/// A manually driven clock for tests.
///
/// Clones share the same time, so a test can hand one clone to the code under
/// test and keep another to [`advance`](Self::advance) it. Time only moves
/// when told to.
#[derive(Debug, Clone)]
pub struct FakeClock {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Debug)]
struct Inner {
    now: Instant,
    wall: Timestamp,
}

impl FakeClock {
    /// A clock starting at an arbitrary monotonic instant and the Unix epoch
    /// on the wall clock.
    #[must_use]
    pub fn new() -> Self {
        Self::with_wall(Timestamp::UNIX_EPOCH)
    }

    /// A clock whose wall time starts at `wall`.
    #[must_use]
    pub fn with_wall(wall: Timestamp) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                now: Instant::now(),
                wall,
            })),
        }
    }

    /// Moves both the monotonic and wall clocks forward by `by`.
    ///
    /// Saturates instead of overflowing on absurdly large durations.
    pub fn advance(&self, by: Duration) {
        let mut inner = self.lock();
        inner.now = inner.now.checked_add(by).unwrap_or(inner.now);
        inner.wall = inner.wall.checked_add(by).unwrap_or(Timestamp::MAX);
    }

    /// Sets the wall clock without touching monotonic time, like an NTP jump.
    pub fn set_wall(&self, wall: Timestamp) {
        self.lock().wall = wall;
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        lock(&self.inner)
    }
}

impl Default for FakeClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Instant {
        self.lock().now
    }

    fn wall_now(&self) -> Timestamp {
        self.lock().wall
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_only_moves_when_advanced() {
        let clock = FakeClock::default();
        let start = clock.now();
        assert_eq!(clock.now(), start);
        assert_eq!(clock.wall_now(), Timestamp::UNIX_EPOCH);

        clock.advance(Duration::from_secs(90));
        assert_eq!(clock.now() - start, Duration::from_secs(90));
        assert_eq!(clock.wall_now().as_second(), 90);
    }

    #[test]
    fn clones_share_time() {
        let clock = FakeClock::new();
        let handle = clock.clone();
        let start = clock.now();
        handle.advance(Duration::from_millis(250));
        assert_eq!(clock.now() - start, Duration::from_millis(250));
    }

    #[test]
    fn set_wall_leaves_monotonic_time_alone() {
        let clock = FakeClock::with_wall(Timestamp::from_second(1_000).unwrap());
        let start = clock.now();
        clock.set_wall(Timestamp::from_second(10).unwrap());
        assert_eq!(clock.wall_now().as_second(), 10);
        assert_eq!(clock.now(), start);
    }

    #[test]
    fn huge_advances_saturate() {
        let clock = FakeClock::new();
        clock.advance(Duration::MAX);
        assert_eq!(clock.wall_now(), Timestamp::MAX);
    }
}
