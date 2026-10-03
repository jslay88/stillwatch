//! The snooze ceiling: the same block counters, measured over `ceiling_minutes`.

use crate::config::{SafetyConfig, StaleConfig};
use crate::detector::threshold::percent;
use crate::stats::{Threshold, ThresholdReason};

/// Unchanged captures that add up to at least `ceiling_minutes`:
/// `ceil(ceiling_minutes * 60 / check_interval_seconds)`, and at least 1.
pub(crate) fn ceiling_checks(stale: &StaleConfig, safety: &SafetyConfig) -> u32 {
    let seconds = u64::from(safety.ceiling_minutes) * 60;
    let checks = seconds.div_ceil(u64::from(stale.check_interval_seconds.max(1)));
    u32::try_from(checks.max(1)).unwrap_or(u32::MAX)
}

/// `ceiling_stale_percent` as a [`Threshold`].
pub(crate) fn ceiling_threshold(safety: &SafetyConfig) -> Threshold {
    Threshold::new(
        percent(safety.ceiling_stale_percent),
        ThresholdReason::Ceiling,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checks(ceiling_minutes: u32, check_interval_seconds: u32) -> u32 {
        let stale = StaleConfig {
            check_interval_seconds,
            ..StaleConfig::default()
        };
        let safety = SafetyConfig {
            ceiling_minutes,
            ..SafetyConfig::default()
        };
        ceiling_checks(&stale, &safety)
    }

    #[test]
    fn defaults_need_thirty_captures() {
        assert_eq!(
            ceiling_checks(&StaleConfig::default(), &SafetyConfig::default()),
            30
        );
    }

    #[test]
    fn partial_intervals_round_up() {
        assert_eq!(checks(7, 45), 10);
        assert_eq!(checks(30, 90), 20);
    }

    #[test]
    fn degenerate_values_still_need_one_capture() {
        assert_eq!(checks(0, 60), 1);
        assert_eq!(checks(1, 0), 60);
    }

    #[test]
    fn threshold_uses_the_ceiling_reason() {
        assert_eq!(
            ceiling_threshold(&SafetyConfig::default()),
            Threshold::new(98, ThresholdReason::Ceiling)
        );
    }
}
