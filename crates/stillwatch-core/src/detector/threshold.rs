//! Picks the stale threshold: `stale_percent`, or `media_stale_percent` while
//! a non-ignored MPRIS player is playing.

use crate::backend::MediaPlayer;
use crate::config::StaleConfig;
use crate::stats::{Threshold, ThresholdReason};

/// The threshold for a capture, given every playing player.
///
/// Players are matched against `media_ignore_players` by
/// [`StaleConfig::media_playing`]. `media_stale_percent = 0` disables the
/// media threshold.
pub(crate) fn select(stale: &StaleConfig, playing: &[MediaPlayer]) -> Threshold {
    let media = stale.media_stale_percent > 0 && stale.media_playing(playing);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::MediaPlayer;

    fn playing(names: &[&str]) -> Vec<MediaPlayer> {
        names.iter().copied().map(MediaPlayer::named).collect()
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
    fn only_ignored_players_keep_the_normal_threshold() {
        let stale = StaleConfig::default();
        for name in ["spotify", "Spotify", "spotify.instance_7"] {
            assert_eq!(
                select(&stale, &playing(&[name])).reason,
                ThresholdReason::Normal,
                "{name}"
            );
        }
    }

    #[test]
    fn an_ignored_identity_keeps_the_normal_threshold() {
        let stale = StaleConfig {
            media_ignore_players: vec!["VLC media player".into()],
            ..StaleConfig::default()
        };
        let vlc = MediaPlayer::with_identity("vlc", "VLC media player");
        assert_eq!(select(&stale, &[vlc]).reason, ThresholdReason::Normal);
        let named = MediaPlayer::with_identity("firefox.instance_1_42", "Mozilla Firefox");
        let by_suffix = StaleConfig {
            media_ignore_players: vec!["firefox".into()],
            ..StaleConfig::default()
        };
        assert_eq!(select(&by_suffix, &[named]).reason, ThresholdReason::Normal);
        assert_eq!(
            select(&stale, &[MediaPlayer::named("mpv")]).reason,
            ThresholdReason::Media
        );
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
