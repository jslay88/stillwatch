//! Turns per-output block states into [`DetectionStats`] and the screen verdict.

use crate::config::StaleRequire;
use crate::stats::{BlockCounts, BlockState, DetectionStats, OutputStats, Threshold};

/// Builds the stats for one capture (or ceiling query) from each output's
/// block states, then applies `require` across them.
///
/// With no outputs the screen is not stale, even with `require = "all"`.
pub(crate) fn summarize<'a>(
    outputs: impl IntoIterator<Item = (&'a str, &'a [BlockState])>,
    threshold: Threshold,
    require: StaleRequire,
) -> DetectionStats {
    let outputs: Vec<OutputStats> = outputs
        .into_iter()
        .map(|(name, states)| {
            OutputStats::from_counts(name, BlockCounts::from_states(states), threshold.percent)
        })
        .collect();
    let stale = !outputs.is_empty()
        && match require {
            StaleRequire::All => outputs.iter().all(|output| output.stale),
            StaleRequire::Any => outputs.iter().any(|output| output.stale),
        };
    DetectionStats {
        outputs,
        threshold,
        stale,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::ThresholdReason;

    const STALE: [BlockState; 2] = [BlockState::Persistent; 2];
    const FRESH: [BlockState; 2] = [BlockState::Changed; 2];
    const THRESHOLD: Threshold = Threshold::new(70, ThresholdReason::Normal);

    #[test]
    fn all_needs_every_output_stale() {
        let mixed = [("A", &STALE[..]), ("B", &FRESH[..])];
        assert!(!summarize(mixed, THRESHOLD, StaleRequire::All).stale);
        let both = [("A", &STALE[..]), ("B", &STALE[..])];
        assert!(summarize(both, THRESHOLD, StaleRequire::All).stale);
    }

    #[test]
    fn any_needs_one_stale_output() {
        let mixed = [("A", &FRESH[..]), ("B", &STALE[..])];
        let stats = summarize(mixed, THRESHOLD, StaleRequire::Any);
        assert!(stats.stale);
        assert_eq!(stats.outputs.len(), 2);
        assert_eq!(stats.threshold, THRESHOLD);
        let neither = [("A", &FRESH[..]), ("B", &FRESH[..])];
        assert!(!summarize(neither, THRESHOLD, StaleRequire::Any).stale);
    }

    #[test]
    fn no_outputs_is_never_stale() {
        for require in [StaleRequire::All, StaleRequire::Any] {
            assert!(!summarize([], THRESHOLD, require).stale);
        }
    }
}
