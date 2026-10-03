//! `[capture]`, `[stale]`, and `[safety]`: how burn-in risk is detected.

use serde::{Deserialize, Serialize};

use crate::backend::MediaPlayer;
use crate::config::limits::{Bounds, DOWNSCALE_WIDTH, LUMA, PERCENT, PERCENT_NONZERO, POSITIVE};
use crate::config::validate::Issues;

/// Which screen capture backend to use.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureBackend {
    /// Pick the best backend the desktop supports.
    #[default]
    Auto,
    /// `org.kde.KWin.ScreenShot2`.
    Kwin,
    /// xdg-desktop-portal `ScreenCast`.
    Portal,
}

/// `[capture]`: capture backend selection.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CaptureConfig {
    /// Capture backend override.
    pub backend: CaptureBackend,
}

/// How many monitored outputs must be stale for the screen to count as stale.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StaleRequire {
    /// Every monitored output must be stale.
    #[default]
    All,
    /// One stale output is enough.
    Any,
}

/// A rectangle on one output that the detector skips, such as a panel clock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IgnoreRegion {
    /// Output connector name, such as `HDMI-A-1`.
    pub output: String,
    /// Left edge in output pixels.
    pub x: u32,
    /// Top edge in output pixels.
    pub y: u32,
    /// Width in output pixels.
    pub w: u32,
    /// Height in output pixels.
    pub h: u32,
}

/// `[stale]`: the per-block persistence detector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StaleConfig {
    /// Seconds between captures while idle.
    pub check_interval_seconds: u32,
    /// Consecutive unchanged captures before a block counts as persistent.
    pub persist_checks: u32,
    /// Percentage (1-100) of counted blocks that must be persistent.
    pub stale_percent: u32,
    /// Threshold used while a non-ignored MPRIS player is playing; 0 disables it.
    pub media_stale_percent: u32,
    /// Audio-only players that shouldn't raise the threshold.
    pub media_ignore_players: Vec<String>,
    /// Largest mean-luma change (0-255) that still counts as unchanged.
    pub luma_delta_threshold: u32,
    /// Blocks darker than this luma (0-255) are excluded; 0 disables it.
    pub ignore_dark_below: u32,
    /// Width in pixels of the downscaled luma grid.
    pub downscale_width: u32,
    /// Block grid as `[cols, rows]`.
    pub block_grid: [u32; 2],
    /// Connector names to monitor; empty means all outputs.
    pub monitored_outputs: Vec<String>,
    /// Whether all or any monitored outputs must be stale.
    pub require: StaleRequire,
    /// Regions the detector skips.
    pub ignore_regions: Vec<IgnoreRegion>,
}

impl Default for StaleConfig {
    fn default() -> Self {
        Self {
            check_interval_seconds: 60,
            persist_checks: 5,
            stale_percent: 70,
            media_stale_percent: 90,
            media_ignore_players: vec!["spotify".to_owned()],
            luma_delta_threshold: 6,
            ignore_dark_below: 16,
            downscale_width: 480,
            block_grid: [16, 16],
            monitored_outputs: Vec::new(),
            require: StaleRequire::default(),
            ignore_regions: Vec::new(),
        }
    }
}

impl StaleConfig {
    /// Seconds a block must stay unchanged before it counts as persistent.
    #[must_use]
    pub fn normal_path_seconds(&self) -> u64 {
        u64::from(self.persist_checks) * u64::from(self.check_interval_seconds)
    }

    /// Whether `name` or `identity` is on `media_ignore_players`.
    ///
    /// The bus-name suffix matches case-insensitively, as the whole name or
    /// as the prefix before a `.` (`firefox` matches `firefox.instance_1_42`).
    /// `identity` matches case-insensitively as a whole string
    /// (`VLC media player`), not as a prefix of that string.
    #[must_use]
    pub fn is_player_ignored(&self, name: &str, identity: &str) -> bool {
        self.media_ignore_players
            .iter()
            .any(|ignored| suffix_matches(name, ignored) || identity_matches(identity, ignored))
    }

    /// Whether any of the `playing` players is not ignored.
    #[must_use]
    pub fn media_playing(&self, playing: &[MediaPlayer]) -> bool {
        playing
            .iter()
            .any(|player| !self.is_player_ignored(&player.name, &player.identity))
    }

    pub(crate) fn validate(&self, issues: &mut Issues) {
        issues.range(
            "stale.check_interval_seconds",
            self.check_interval_seconds,
            POSITIVE,
        );
        issues.range("stale.persist_checks", self.persist_checks, POSITIVE);
        issues.range("stale.stale_percent", self.stale_percent, PERCENT_NONZERO);
        issues.range(
            "stale.media_stale_percent",
            self.media_stale_percent,
            PERCENT,
        );
        issues.entries_not_blank("stale.media_ignore_players", &self.media_ignore_players);
        issues.range(
            "stale.luma_delta_threshold",
            self.luma_delta_threshold,
            LUMA,
        );
        issues.range("stale.ignore_dark_below", self.ignore_dark_below, LUMA);
        issues.range(
            "stale.downscale_width",
            self.downscale_width,
            DOWNSCALE_WIDTH,
        );
        self.validate_block_grid(issues);
        issues.entries_not_blank("stale.monitored_outputs", &self.monitored_outputs);
        for (index, region) in self.ignore_regions.iter().enumerate() {
            let key = format!("stale.ignore_regions[{index}]");
            issues.not_blank(&format!("{key}.output"), &region.output);
            issues.range(&format!("{key}.w"), region.w, POSITIVE);
            issues.range(&format!("{key}.h"), region.h, POSITIVE);
        }
    }

    fn validate_block_grid(&self, issues: &mut Issues) {
        // The downscaled height depends on the output's aspect ratio, so the
        // width is the only bound known before capture.
        let max = self.downscale_width.max(POSITIVE.min);
        let allowed = Bounds::new(POSITIVE.min, max);
        for (index, (name, value)) in ["cols", "rows"].iter().zip(self.block_grid).enumerate() {
            if !allowed.contains(value) {
                issues.push(
                    format!("stale.block_grid[{index}]"),
                    format!("{name} must be between 1 and downscale_width ({max}), got {value}"),
                );
            }
        }
    }
}

/// Case-insensitive bus-name match: the whole suffix, or a prefix ending at `.`.
fn suffix_matches(name: &str, ignored: &str) -> bool {
    name.get(..ignored.len()).is_some_and(|head| {
        head.eq_ignore_ascii_case(ignored)
            && matches!(name.as_bytes().get(ignored.len()), None | Some(b'.'))
    })
}

/// Case-insensitive whole-string match against `Identity`. An empty identity
/// matches nothing, including a blank ignore entry.
fn identity_matches(identity: &str, ignored: &str) -> bool {
    !identity.is_empty() && identity.eq_ignore_ascii_case(ignored)
}

/// `[safety]`: the snooze ceiling.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SafetyConfig {
    /// Whether the ceiling can end a snooze early.
    pub ceiling_enabled: bool,
    /// Minutes blocks must stay unchanged for the ceiling to trigger.
    pub ceiling_minutes: u32,
    /// Percentage (1-100) of counted blocks unchanged for `ceiling_minutes`.
    pub ceiling_stale_percent: u32,
    /// Whether the ceiling also applies while paused.
    pub ceiling_during_pause: bool,
}

impl Default for SafetyConfig {
    fn default() -> Self {
        Self {
            ceiling_enabled: true,
            ceiling_minutes: 30,
            ceiling_stale_percent: 98,
            ceiling_during_pause: false,
        }
    }
}

impl SafetyConfig {
    pub(crate) fn validate(&self, stale: &StaleConfig, issues: &mut Issues) {
        issues.range(
            "safety.ceiling_stale_percent",
            self.ceiling_stale_percent,
            PERCENT_NONZERO,
        );
        let ceiling_seconds = u64::from(self.ceiling_minutes) * 60;
        let normal_seconds = stale.normal_path_seconds();
        if self.ceiling_enabled && ceiling_seconds <= normal_seconds {
            issues.push(
                "safety.ceiling_minutes",
                format!(
                    "must be longer than the normal stale path \
                     (persist_checks * check_interval_seconds = {normal_seconds}s), \
                     got {ceiling_seconds}s"
                ),
            );
        }
    }
}
