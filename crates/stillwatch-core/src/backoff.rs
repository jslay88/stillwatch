//! Reconnect back-off.
//!
//! Backends that lose their connection (Wayland, D-Bus, evdev, the portal)
//! retry with exponentially growing delays. [`Backoff`] is only the
//! bookkeeping; the daemon does the sleeping.

use std::time::Duration;

/// The shape of a back-off schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackoffPolicy {
    /// Delay before the first retry.
    pub initial: Duration,
    /// Upper bound on any delay.
    pub max: Duration,
    /// Each retry waits this many times longer than the one before. `1` keeps
    /// the delay constant; `0` is treated as `1`.
    pub multiplier: u32,
    /// An attempt that ran at least this long counts as healthy, so the
    /// failure that ended it starts the schedule over from `initial`.
    pub reset_after: Duration,
}

impl Default for BackoffPolicy {
    /// 1 s, doubling up to 60 s; a connection that lasted a minute resets it.
    fn default() -> Self {
        Self {
            initial: Duration::from_secs(1),
            max: Duration::from_secs(60),
            multiplier: 2,
            reset_after: Duration::from_secs(60),
        }
    }
}

/// Exponential back-off state for one backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backoff {
    policy: BackoffPolicy,
    next: Duration,
    failures: u32,
}

impl Backoff {
    /// A fresh schedule following `policy`.
    #[must_use]
    pub fn new(policy: BackoffPolicy) -> Self {
        Self {
            policy,
            next: policy.initial.min(policy.max),
            failures: 0,
        }
    }

    /// The policy this schedule follows.
    #[must_use]
    pub const fn policy(&self) -> BackoffPolicy {
        self.policy
    }

    /// Consecutive failures since the last reset.
    #[must_use]
    pub const fn failures(&self) -> u32 {
        self.failures
    }

    /// Records a failure and returns how long to wait before retrying.
    pub fn next_delay(&mut self) -> Duration {
        let delay = self.next;
        self.next = delay
            .saturating_mul(self.policy.multiplier.max(1))
            .min(self.policy.max);
        self.failures = self.failures.saturating_add(1);
        delay
    }

    /// Records a failed attempt that ran for `ran_for` and returns how long to
    /// wait before retrying. An attempt that lasted `reset_after` or longer
    /// was healthy, so the schedule starts over first.
    pub fn after_attempt(&mut self, ran_for: Duration) -> Duration {
        if ran_for >= self.policy.reset_after {
            self.reset();
        }
        self.next_delay()
    }

    /// Starts the schedule over, as after a success.
    pub fn reset(&mut self) {
        *self = Self::new(self.policy);
    }
}

impl Default for Backoff {
    fn default() -> Self {
        Self::new(BackoffPolicy::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::{Clock, FakeClock};

    fn policy(initial: u64, max: u64, multiplier: u32, reset_after: u64) -> BackoffPolicy {
        BackoffPolicy {
            initial: Duration::from_secs(initial),
            max: Duration::from_secs(max),
            multiplier,
            reset_after: Duration::from_secs(reset_after),
        }
    }

    fn delays(backoff: &mut Backoff, n: usize) -> Vec<u64> {
        (0..n).map(|_| backoff.next_delay().as_secs()).collect()
    }

    #[test]
    fn default_doubles_from_one_second_up_to_a_minute() {
        let mut backoff = Backoff::default();
        assert_eq!(backoff.policy(), BackoffPolicy::default());
        assert_eq!(delays(&mut backoff, 8), [1, 2, 4, 8, 16, 32, 60, 60]);
        assert_eq!(backoff.failures(), 8);
    }

    #[test]
    fn reset_starts_over() {
        let mut backoff = Backoff::new(policy(2, 30, 3, 10));
        assert_eq!(delays(&mut backoff, 3), [2, 6, 18]);
        backoff.reset();
        assert_eq!(backoff.failures(), 0);
        assert_eq!(delays(&mut backoff, 3), [2, 6, 18]);
    }

    #[test]
    fn multiplier_one_or_zero_keeps_the_delay_constant() {
        for multiplier in [0, 1] {
            let mut backoff = Backoff::new(policy(5, 60, multiplier, 60));
            assert_eq!(delays(&mut backoff, 3), [5, 5, 5]);
        }
    }

    #[test]
    fn initial_above_max_is_capped() {
        let mut backoff = Backoff::new(policy(90, 60, 2, 60));
        assert_eq!(delays(&mut backoff, 2), [60, 60]);
    }

    #[test]
    fn huge_values_saturate_instead_of_overflowing() {
        let mut backoff = Backoff::new(BackoffPolicy {
            initial: Duration::MAX,
            max: Duration::MAX,
            multiplier: u32::MAX,
            reset_after: Duration::MAX,
        });
        assert_eq!(backoff.next_delay(), Duration::MAX);
        assert_eq!(backoff.next_delay(), Duration::MAX);
    }

    #[test]
    fn failure_count_saturates() {
        let mut backoff = Backoff {
            failures: u32::MAX,
            ..Backoff::default()
        };
        backoff.next_delay();
        assert_eq!(backoff.failures(), u32::MAX);
    }

    #[test]
    fn short_attempts_keep_growing_and_healthy_ones_reset() {
        let clock = FakeClock::new();
        let mut backoff = Backoff::new(policy(1, 60, 2, 30));
        let mut attempt = |ran_for: u64| {
            let started = clock.now();
            clock.advance(Duration::from_secs(ran_for));
            backoff.after_attempt(clock.now() - started).as_secs()
        };
        assert_eq!(attempt(0), 1);
        assert_eq!(attempt(5), 2);
        assert_eq!(attempt(29), 4);
        assert_eq!(attempt(30), 1);
        assert_eq!(attempt(1), 2);
        assert_eq!(attempt(3_600), 1);
    }
}
