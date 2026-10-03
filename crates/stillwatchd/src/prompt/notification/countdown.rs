//! The remaining time the prompt shows, one 10 s step per update.

use std::time::Duration;

/// How much the shown countdown drops per update.
pub(crate) const STEP: Duration = Duration::from_secs(10);

/// Remaining time on the prompt, counted down in [`STEP`]s.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Countdown {
    remaining: Duration,
}

impl Countdown {
    pub(crate) const fn new(total: Duration) -> Self {
        Self { remaining: total }
    }

    pub(crate) const fn remaining(self) -> Duration {
        self.remaining
    }

    /// Moves one step on. `false`, leaving it unchanged, once another step
    /// would go below zero: the last value stays up until the state machine's
    /// own countdown acts.
    pub(crate) fn advance(&mut self) -> bool {
        match self.remaining.checked_sub(STEP) {
            Some(remaining) => {
                self.remaining = remaining;
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steps(total: Duration) -> Vec<u64> {
        let mut countdown = Countdown::new(total);
        let mut shown = vec![countdown.remaining().as_secs()];
        while countdown.advance() {
            shown.push(countdown.remaining().as_secs());
        }
        shown
    }

    #[test]
    fn counts_down_to_zero_in_ten_second_steps() {
        assert_eq!(steps(Duration::from_mins(1)), [60, 50, 40, 30, 20, 10, 0]);
    }

    #[test]
    fn stops_at_the_last_step_above_zero() {
        assert_eq!(steps(Duration::from_secs(25)), [25, 15, 5]);
        assert_eq!(steps(Duration::from_secs(5)), [5]);
    }

    #[test]
    fn advancing_past_the_end_changes_nothing() {
        let mut countdown = Countdown::new(Duration::from_secs(5));
        assert!(!countdown.advance());
        assert!(!countdown.advance());
        assert_eq!(countdown.remaining(), Duration::from_secs(5));
    }
}
