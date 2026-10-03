use std::ops::RangeInclusive;
use std::path::Path;

use toml::Table;

use super::{Config, MigrationStep, rename_key};

/// A numeric key whose only rule is an allowed range, given the default config
/// for every other key. Setters pin anything the range depends on.
pub(crate) struct RangeRule {
    pub(crate) key: &'static str,
    pub(crate) allowed: RangeInclusive<u32>,
    pub(crate) set: fn(&mut Config, u32),
}

pub(crate) const RANGE_RULES: &[RangeRule] = &[
    RangeRule {
        key: "idle.input_idle_minutes",
        allowed: 1..=u32::MAX,
        set: |c, v| c.idle.input_idle_minutes = v,
    },
    RangeRule {
        key: "activity.gamepad_deadzone_percent",
        allowed: 0..=100,
        set: |c, v| c.activity.gamepad_deadzone_percent = v,
    },
    RangeRule {
        key: "stale.check_interval_seconds",
        allowed: 1..=u32::MAX,
        set: |c, v| {
            c.stale.check_interval_seconds = v;
            c.safety.ceiling_enabled = false;
        },
    },
    RangeRule {
        key: "stale.persist_checks",
        allowed: 1..=u32::MAX,
        set: |c, v| {
            c.stale.persist_checks = v;
            c.safety.ceiling_enabled = false;
        },
    },
    RangeRule {
        key: "stale.stale_percent",
        allowed: 1..=100,
        set: |c, v| c.stale.stale_percent = v,
    },
    RangeRule {
        key: "stale.media_stale_percent",
        allowed: 0..=100,
        set: |c, v| c.stale.media_stale_percent = v,
    },
    RangeRule {
        key: "stale.luma_delta_threshold",
        allowed: 0..=255,
        set: |c, v| c.stale.luma_delta_threshold = v,
    },
    RangeRule {
        key: "stale.ignore_dark_below",
        allowed: 0..=255,
        set: |c, v| c.stale.ignore_dark_below = v,
    },
    RangeRule {
        key: "stale.downscale_width",
        allowed: 16..=u32::MAX,
        set: |c, v| {
            c.stale.downscale_width = v;
            c.stale.block_grid = [1, 1];
        },
    },
    RangeRule {
        key: "stale.block_grid[0]",
        allowed: 1..=480,
        set: |c, v| {
            c.stale.downscale_width = 480;
            c.stale.block_grid[0] = v;
        },
    },
    RangeRule {
        key: "stale.block_grid[1]",
        allowed: 1..=480,
        set: |c, v| {
            c.stale.downscale_width = 480;
            c.stale.block_grid[1] = v;
        },
    },
    RangeRule {
        key: "safety.ceiling_stale_percent",
        allowed: 1..=100,
        set: |c, v| c.safety.ceiling_stale_percent = v,
    },
    RangeRule {
        key: "prompt.countdown_seconds",
        allowed: 1..=u32::MAX,
        set: |c, v| c.prompt.countdown_seconds = v,
    },
    RangeRule {
        key: "prompt.answer_grace_seconds",
        allowed: 1..=120,
        set: |c, v| c.prompt.answer_grace_seconds = v,
    },
    RangeRule {
        key: "prompt.custom_min_minutes",
        allowed: 1..=u32::MAX,
        set: |c, v| {
            c.prompt.custom_min_minutes = v;
            c.prompt.custom_max_minutes = u32::MAX;
            c.prompt.snooze_presets_minutes.clear();
        },
    },
    RangeRule {
        key: "action.dim_percent",
        allowed: 0..=100,
        set: |c, v| c.action.dim_percent = v,
    },
    RangeRule {
        key: "panel_care.reminder_hours",
        allowed: 1..=u32::MAX,
        set: |c, v| c.panel_care.reminder_hours = v,
    },
    RangeRule {
        key: "history.max_entries",
        allowed: 1..=u32::MAX,
        set: |c, v| c.history.max_entries = v,
    },
];

/// A stand-in v0 -> v1 step: `stale.threshold_percent` became `stale.stale_percent`.
pub(crate) const SYNTHETIC_V0_TO_V1: MigrationStep = MigrationStep {
    from: 0,
    apply: rename_threshold,
};

fn rename_threshold(table: &mut Table) -> Vec<String> {
    rename_key(table, "stale", "threshold_percent", "stale_percent")
        .into_iter()
        .collect()
}

pub(crate) fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/config")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

pub(crate) fn issue_keys(config: &Config) -> Vec<String> {
    config
        .validate()
        .err()
        .unwrap_or_default()
        .into_iter()
        .map(|issue| issue.key)
        .collect()
}
