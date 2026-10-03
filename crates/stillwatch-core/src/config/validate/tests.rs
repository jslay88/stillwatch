use crate::config::test_support::{RANGE_RULES, issue_keys};
use crate::config::{ActionMode, Config, IgnoreRegion, ValidationIssue};

fn with(edit: impl FnOnce(&mut Config)) -> Config {
    let mut config = Config::default();
    edit(&mut config);
    config
}

fn region(output: &str, w: u32, h: u32) -> IgnoreRegion {
    IgnoreRegion {
        output: output.to_owned(),
        x: 10,
        y: 20,
        w,
        h,
    }
}

fn assert_valid(config: &Config) {
    assert_eq!(config.validate(), Ok(()));
}

fn assert_only(config: &Config, key: &str) {
    assert_eq!(issue_keys(config), [key]);
}

#[test]
fn numeric_range_boundaries() {
    for rule in RANGE_RULES {
        let (min, max) = (*rule.allowed.start(), *rule.allowed.end());
        for value in [min, max] {
            let config = with(|c| (rule.set)(c, value));
            assert_valid(&config);
        }
        let outside = [min.checked_sub(1), max.checked_add(1)];
        for value in outside.into_iter().flatten() {
            let config = with(|c| (rule.set)(c, value));
            assert_eq!(issue_keys(&config), [rule.key], "{} = {value}", rule.key);
        }
    }
}

#[test]
fn range_message_names_bounds_and_value() {
    let issues = with(|c| c.stale.stale_percent = 0).validate().unwrap_err();
    assert_eq!(issues[0].message, "must be between 1 and 100, got 0");
    let issues = with(|c| c.history.max_entries = 0).validate().unwrap_err();
    assert_eq!(issues[0].message, "must be at least 1, got 0");
}

#[test]
fn block_grid_message_names_the_axis() {
    let issues = with(|c| c.stale.block_grid = [16, 500])
        .validate()
        .unwrap_err();
    assert_eq!(
        issues[0].message,
        "rows must be between 1 and downscale_width (480), got 500"
    );
}

#[test]
fn block_grid_follows_downscale_width() {
    let config = with(|c| {
        c.stale.downscale_width = 32;
        c.stale.block_grid = [32, 33];
    });
    assert_only(&config, "stale.block_grid[1]");
}

#[test]
fn ceiling_must_outlast_the_normal_path() {
    // Defaults: persist_checks 5 * check_interval_seconds 60 = 300s.
    assert_only(
        &with(|c| c.safety.ceiling_minutes = 5),
        "safety.ceiling_minutes",
    );
    assert_only(
        &with(|c| c.safety.ceiling_minutes = 0),
        "safety.ceiling_minutes",
    );
    assert_valid(&with(|c| c.safety.ceiling_minutes = 6));
    assert_only(
        &with(|c| c.stale.check_interval_seconds = 360),
        "safety.ceiling_minutes",
    );
}

#[test]
fn ceiling_rule_is_skipped_when_disabled() {
    assert_valid(&with(|c| {
        c.safety.ceiling_enabled = false;
        c.safety.ceiling_minutes = 1;
    }));
}

#[test]
fn ceiling_message_shows_both_durations() {
    let issues = with(|c| c.safety.ceiling_minutes = 5)
        .validate()
        .unwrap_err();
    let message = &issues[0].message;
    assert!(message.contains("= 300s"), "{message}");
    assert!(message.ends_with("got 300s"), "{message}");
}

#[test]
fn ceiling_rule_handles_huge_values() {
    let config = with(|c| {
        c.stale.persist_checks = u32::MAX;
        c.stale.check_interval_seconds = u32::MAX;
        c.safety.ceiling_minutes = u32::MAX;
    });
    assert_only(&config, "safety.ceiling_minutes");
}

#[test]
fn snooze_presets_required_without_custom() {
    let empty_no_custom = with(|c| {
        c.prompt.snooze_presets_minutes.clear();
        c.prompt.allow_custom = false;
    });
    assert_only(&empty_no_custom, "prompt.snooze_presets_minutes");
    assert_valid(&with(|c| c.prompt.snooze_presets_minutes.clear()));
    assert_valid(&with(|c| c.prompt.allow_custom = false));
}

#[test]
fn snooze_presets_within_custom_range() {
    let config = with(|c| {
        c.prompt.custom_min_minutes = 15;
        c.prompt.custom_max_minutes = 60;
        c.prompt.snooze_presets_minutes = vec![15, 61, 60, 14];
    });
    assert_eq!(
        issue_keys(&config),
        [
            "prompt.snooze_presets_minutes[1]",
            "prompt.snooze_presets_minutes[3]"
        ]
    );
}

#[test]
fn custom_min_must_not_exceed_max() {
    let config = with(|c| {
        c.prompt.custom_min_minutes = 30;
        c.prompt.custom_max_minutes = 20;
    });
    assert_only(&config, "prompt.custom_max_minutes");
    assert_valid(&with(|c| {
        c.prompt.custom_min_minutes = 20;
        c.prompt.custom_max_minutes = 20;
        c.prompt.snooze_presets_minutes = vec![20];
    }));
}

#[test]
fn command_required_in_command_mode() {
    let command_mode = |command: &str| {
        with(|c| {
            c.action.mode = ActionMode::Command;
            c.action.command = command.to_owned();
        })
    };
    assert_only(&command_mode(""), "action.command");
    assert_only(&command_mode("  \t"), "action.command");
    assert_valid(&command_mode("loginctl lock-session"));
    assert_valid(&with(|c| c.action.mode = ActionMode::LockAndBlank));
}

#[test]
fn ignore_regions_need_area_and_output() {
    let config = with(|c| {
        c.stale.ignore_regions = vec![
            region("HDMI-A-1", 200, 40),
            region("HDMI-A-1", 0, 40),
            region("HDMI-A-1", 200, 0),
            region(" ", 1, 1),
        ];
    });
    assert_eq!(
        issue_keys(&config),
        [
            "stale.ignore_regions[1].w",
            "stale.ignore_regions[2].h",
            "stale.ignore_regions[3].output"
        ]
    );
}

#[test]
fn name_lists_reject_blank_entries() {
    let config = with(|c| {
        c.activity.gamepad_ignore_devices = vec!["8BitDo".to_owned(), String::new()];
        c.stale.media_ignore_players = vec![" ".to_owned()];
        c.stale.monitored_outputs = vec!["DP-1".to_owned(), "\t".to_owned()];
    });
    assert_eq!(
        issue_keys(&config),
        [
            "activity.gamepad_ignore_devices[1]",
            "stale.media_ignore_players[0]",
            "stale.monitored_outputs[1]"
        ]
    );
}

#[test]
fn version_must_be_current() {
    assert_only(&with(|c| c.version = 0), "version");
}

#[test]
fn reports_every_issue_at_once() {
    let config = with(|c| {
        c.idle.input_idle_minutes = 0;
        c.stale.stale_percent = 101;
        c.safety.ceiling_stale_percent = 0;
        c.action.mode = ActionMode::Command;
        c.history.max_entries = 0;
    });
    assert_eq!(
        issue_keys(&config),
        [
            "idle.input_idle_minutes",
            "stale.stale_percent",
            "safety.ceiling_stale_percent",
            "action.command",
            "history.max_entries"
        ]
    );
}

#[test]
fn issue_display_is_key_then_message() {
    let issue = ValidationIssue {
        key: "stale.stale_percent".to_owned(),
        message: "must be between 1 and 100, got 0".to_owned(),
    };
    assert_eq!(
        issue.to_string(),
        "stale.stale_percent: must be between 1 and 100, got 0"
    );
}
