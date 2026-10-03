//! Injectable time: a [`Clock`] trait, the real [`SystemClock`], a shared
//! [`FakeClock`] for tests, and a pure [`TimerQueue`].
//!
//! The state machine never sleeps. It asks for timers with
//! `Command::SetTimer`, the daemon keeps them in a [`TimerQueue`], and fired
//! timers come back as `Event::Timer`.

mod fake;
mod timers;

use std::time::Instant;

use jiff::Timestamp;

pub use fake::FakeClock;
pub use timers::{TimerId, TimerQueue};

/// A source of the current time.
pub trait Clock: Send + Sync {
    /// Monotonic now, for timers and durations.
    fn now(&self) -> Instant;

    /// Wall-clock now, for history timestamps. May jump.
    fn wall_now(&self) -> Timestamp;
}

/// The real system clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn wall_now(&self) -> Timestamp {
        Timestamp::now()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_clock_moves_forward() {
        let clock = SystemClock;
        let before = clock.now();
        let wall = clock.wall_now();
        assert!(clock.now() >= before);
        assert!(wall > Timestamp::UNIX_EPOCH);
    }

    #[test]
    fn clocks_are_usable_as_trait_objects() {
        let clocks: [Box<dyn Clock>; 2] = [Box::new(SystemClock), Box::new(FakeClock::new())];
        for clock in &clocks {
            assert!(clock.now() <= clock.now());
        }
    }
}
