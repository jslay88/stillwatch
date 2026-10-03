use toml::{Table, Value};

use super::*;
use crate::config::{CaptureBackend, IgnoreRegion};
use crate::schema::{Control, Setting, default_table, lookup, settings};

/// A different value for `setting`, of the right type (and a real choice for
/// enums), so the result still deserializes.
fn other_value(setting: &Setting, current: &Value) -> Value {
    match (current, setting.control) {
        (Value::Integer(n), _) => Value::Integer(n + 1),
        (Value::Boolean(b), _) => Value::Boolean(!b),
        (Value::String(s), Control::Enum { choices }) => {
            let choice = choices.iter().find(|choice| choice.value != s).unwrap();
            Value::String(choice.value.to_owned())
        }
        (Value::String(s), _) => Value::String(format!("{s}x")),
        (Value::Array(items), Control::GridSize { .. }) => {
            let mut items = items.clone();
            items[0] = Value::Integer(items[0].as_integer().unwrap() + 1);
            Value::Array(items)
        }
        (Value::Array(items), control) => {
            let item = match control {
                Control::IntList { .. } => Value::Integer(1),
                Control::RegionEditor => Value::Table(
                    toml::from_str("output = \"DP-1\"\nx = 0\ny = 0\nw = 1\nh = 1").unwrap(),
                ),
                _ => Value::String("x".to_owned()),
            };
            let mut items = items.clone();
            items.push(item);
            Value::Array(items)
        }
        (value, control) => panic!("no edit for {} ({value:?}, {control:?})", setting.key),
    }
}

fn with_changed(setting: &Setting) -> Config {
    let mut table = default_table().unwrap();
    let current = lookup(&table, setting.key).unwrap().clone();
    let (section, name) = setting.key.rsplit_once('.').unwrap_or(("", setting.key));
    let target: &mut Table = if section.is_empty() {
        &mut table
    } else {
        table[section].as_table_mut().unwrap()
    };
    target.insert(name.to_owned(), other_value(setting, &current));
    table.try_into().unwrap()
}

#[test]
fn identical_configs_have_no_changes() {
    let changes = ConfigChanges::between(&Config::default(), &Config::default());
    assert!(changes.is_empty());
    assert_eq!(changes, ConfigChanges::default());
    assert!(!changes.resets_detection());
    assert!(!changes.history_changed());
}

#[test]
fn every_setting_is_compared_and_classified() {
    let old = Config::default();
    for setting in settings() {
        let changes = ConfigChanges::between(&old, &with_changed(setting));
        let keys: Vec<_> = changes.keys().collect();
        assert_eq!(keys, [setting.key], "changing {}", setting.key);
        assert!(changes.contains(setting.key));
        assert_eq!(
            changes.resets_detection(),
            setting.resets_detection,
            "{} resets detection",
            setting.key
        );
        assert_eq!(
            changes.history_changed(),
            setting.key.starts_with("history."),
            "{} changes history",
            setting.key
        );
    }
}

#[test]
fn only_backend_grid_and_monitored_outputs_reset_detection() {
    let old = Config::default();
    let resetting: Vec<_> = settings()
        .filter(|setting| ConfigChanges::between(&old, &with_changed(setting)).resets_detection())
        .map(|setting| setting.key)
        .collect();
    assert_eq!(
        resetting,
        [
            "capture.backend",
            "stale.block_grid",
            "stale.monitored_outputs"
        ]
    );
}

#[test]
fn several_changes_come_back_in_schema_order() {
    let old = Config::default();
    let mut new = old.clone();
    new.history.max_entries = 10;
    new.stale.stale_percent = 50;
    new.capture.backend = CaptureBackend::Portal;
    let changes = ConfigChanges::between(&old, &new);
    let keys: Vec<_> = changes.keys().collect();
    assert_eq!(
        keys,
        [
            "capture.backend",
            "stale.stale_percent",
            "history.max_entries"
        ]
    );
    assert_eq!(changes.settings().count(), 3);
    assert!(changes.resets_detection());
    assert!(changes.history_changed());
    assert!(changes.section_changed("stale"));
    assert!(!changes.section_changed("prompt"));
    assert!(!changes.contains("stale.block_grid"));
}

#[test]
fn live_keys_dont_reset_detection() {
    let old = Config::default();
    let mut new = old.clone();
    new.stale.ignore_regions.push(IgnoreRegion {
        output: "DP-1".into(),
        x: 0,
        y: 0,
        w: 10,
        h: 10,
    });
    new.stale.luma_delta_threshold = 20;
    let changes = ConfigChanges::between(&old, &new);
    assert!(!changes.is_empty());
    assert!(!changes.resets_detection());
}

#[test]
fn reordering_monitored_outputs_counts_as_a_change() {
    let mut old = Config::default();
    old.stale.monitored_outputs = vec!["DP-1".into(), "HDMI-A-1".into()];
    let mut new = old.clone();
    new.stale.monitored_outputs.reverse();
    assert!(ConfigChanges::between(&old, &new).resets_detection());
}

#[test]
fn top_level_keys_are_in_the_unnamed_section() {
    let old = Config::default();
    let mut new = old.clone();
    new.version += 1;
    let changes = ConfigChanges::between(&old, &new);
    assert!(changes.section_changed(""));
    assert!(!changes.section_changed("idle"));
}
