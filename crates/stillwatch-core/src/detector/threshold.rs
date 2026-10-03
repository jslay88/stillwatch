//! Picks the stale threshold: `stale_percent`, or `media_stale_percent` while
//! a non-ignored MPRIS player is playing.

use crate::config::StaleConfig;
use crate::stats::{Threshold, ThresholdReason};

/// The threshold for a capture, given the names of every playing player.
///
/// A player is ignored when any `media_ignore_players` entry is a
/// case-insensitive substring of its name, so `"spotify"` also matches
/// `org.mpris.MediaPlayer2.spotify`. `media_stale_percent = 0` disables the
/// media threshold.
pub(crate) fn select(stale: &StaleConfig, playing: &[String]) -> Threshold {
    let media = stale.media_stale_percent > 0
        && playing
            .iter()
            .any(|player| !is_ignored(player, &stale.media_ignore_players));
    if media {
        Threshold::new(percent(stale.media_stale_percent), ThresholdReason::Media)
    } else {
        Threshold::new(percent(stale.stale_percent), ThresholdReason::Normal)
    }
}

/// Clamps a validated 0-100 config percentage into a [`Threshold`] percent.
pub(crate) fn percent(value: u32) -> u8 {
    u8::try_from(value.min(100)).unwrap_or(100)
}

fn is_ignored(player: &str, ignore: &[String]) -> bool {
    let player = player.to_lowercase();
    ignore
        .iter()
        .any(|entry| player.contains(&entry.to_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playing(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn nothing_playing_uses_the_normal_threshold() {
        let threshold = select(&StaleConfig::default(), &[]);
        assert_eq!(threshold, Threshold::new(70, ThresholdReason::Normal));
    }

    #[test]
    fn a_non_ignored_player_uses_the_media_threshold() {
        let threshold = select(&StaleConfig::default(), &playing(&["spotify", "mpv"]));
        assert_eq!(threshold, Threshold::new(90, ThresholdReason::Media));
    }

    #[test]
    fn ignored_players_match_case_insensitive_substrings() {
        let stale = StaleConfig::default();
        for name in ["spotify", "Spotify", "org.mpris.MediaPlayer2.spotify"] {
            assert_eq!(
                select(&stale, &playing(&[name])).reason,
                ThresholdReason::Normal,
                "{name}"
            );
        }
    }

    #[test]
    fn zero_media_percent_disables_the_media_threshold() {
        let stale = StaleConfig {
            media_stale_percent: 0,
            ..StaleConfig::default()
        };
        let threshold = select(&stale, &playing(&["mpv"]));
        assert_eq!(threshold, Threshold::new(70, ThresholdReason::Normal));
    }

    #[test]
    fn percent_clamps_out_of_range_values() {
        assert_eq!(percent(0), 0);
        assert_eq!(percent(100), 100);
        assert_eq!(percent(u32::MAX), 100);
    }
}
