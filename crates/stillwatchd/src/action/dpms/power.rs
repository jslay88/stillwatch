//! What `org_kde_kwin_dpms` modes mean, and which changes are worth an
//! event.

use std::collections::HashMap;

use wayland_protocols_plasma::dpms::client::org_kde_kwin_dpms::Mode;

/// Whether a DPMS mode shows content. `standby`, `suspend`, and `off` all
/// count as off; unknown modes are `None`.
#[must_use]
pub fn is_on(mode: u32) -> Option<bool> {
    match Mode::try_from(mode).ok()? {
        Mode::On => Some(true),
        Mode::Standby | Mode::Suspend | Mode::Off => Some(false),
        _ => None,
    }
}

/// The last power state reported for each output, so each output's first
/// known state is reported once and after that only changes are.
#[derive(Debug, Default)]
pub struct PowerTracker {
    reported: HashMap<String, bool>,
}

impl PowerTracker {
    /// Records `on` for `output` and returns whether it differs from what was
    /// last reported.
    pub fn update(&mut self, output: &str, on: bool) -> bool {
        self.reported.insert(output.to_owned(), on) != Some(on)
    }

    /// Forgets `output`, so if it comes back its state is reported again.
    pub fn forget(&mut self, output: &str) {
        self.reported.remove(output);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_on_shows_content() {
        assert_eq!(is_on(Mode::On.into()), Some(true));
        for off in [Mode::Standby, Mode::Suspend, Mode::Off] {
            assert_eq!(is_on(off.into()), Some(false));
        }
        assert_eq!(is_on(7), None);
    }

    #[test]
    fn reports_the_first_state_then_only_changes() {
        let mut tracker = PowerTracker::default();
        assert!(tracker.update("HDMI-A-1", true));
        assert!(!tracker.update("HDMI-A-1", true));
        assert!(tracker.update("HDMI-A-1", false));
        assert!(!tracker.update("HDMI-A-1", false));
        assert!(tracker.update("DP-1", false));
        assert!(tracker.update("HDMI-A-1", true));
    }

    #[test]
    fn a_forgotten_output_is_reported_again() {
        let mut tracker = PowerTracker::default();
        assert!(tracker.update("DP-1", true));
        tracker.forget("DP-1");
        assert!(tracker.update("DP-1", true));
    }
}
