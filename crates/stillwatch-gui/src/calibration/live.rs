//! When the calibration page should be probing.

use stillwatch_ipc::probe::MIN_PROBE_INTERVAL_MS;

use crate::shell::DaemonCall;

use super::{Calibration, ProbePace};

/// Milliseconds between samples for `pace`.
///
/// `check_seconds` is `stale.check_interval_seconds`. The daemon rejects
/// anything shorter than [`MIN_PROBE_INTERVAL_MS`].
#[must_use]
pub fn interval_ms(pace: ProbePace, check_seconds: u32) -> u32 {
    let ms = match pace {
        ProbePace::OneSecond => 1_000,
        ProbePace::FiveSeconds => 5_000,
        ProbePace::CheckInterval => check_interval_ms(check_seconds),
    };
    ms.max(MIN_PROBE_INTERVAL_MS)
}

/// `check_seconds` as milliseconds, saturating.
#[must_use]
pub fn check_interval_ms(check_seconds: u32) -> u32 {
    check_seconds.saturating_mul(1_000)
}

/// `StartProbe` or `StopProbe` when the page's visibility or interval changed.
///
/// A running probe is left alone when the daemon drops: the name leaving the
/// bus already stops it, and a `StopProbe` call would only report that nothing
/// is running.
#[must_use]
pub fn next_call(
    cal: &mut Calibration,
    visible: bool,
    daemon_up: bool,
    interval_ms: u32,
) -> Option<DaemonCall> {
    if visible && daemon_up {
        if !cal.running || cal.sent_ms != interval_ms {
            cal.running = true;
            cal.sent_ms = interval_ms;
            return Some(DaemonCall::StartProbe { interval_ms });
        }
        return None;
    }
    let stop = cal.running && daemon_up;
    if cal.running || !visible {
        cal.view = None;
        cal.drag = None;
    }
    cal.running = false;
    stop.then_some(DaemonCall::StopProbe)
}
