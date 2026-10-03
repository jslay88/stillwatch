use std::collections::BTreeSet;

use toml::{Table, Value};

use super::*;
use crate::config::Bounds;
use crate::config::limits::{ANY, POSITIVE};

/// Config keys without a setting, and settings without a config key.
fn coverage_gaps<'a>(
    schema: impl IntoIterator<Item = &'a str>,
    config: &[String],
) -> (Vec<String>, Vec<String>) {
    let schema: BTreeSet<&str> = schema.into_iter().collect();
    let config: BTreeSet<&str> = config.iter().map(String::as_str).collect();
    let missing = config
        .difference(&schema)
        .map(|k| (*k).to_owned())
        .collect();
    let unknown = schema
        .difference(&config)
        .map(|k| (*k).to_owned())
        .collect();
    (missing, unknown)
}

fn config_keys() -> Vec<String> {
    leaf_keys(&default_table().unwrap())
}

fn schema_keys() -> impl Iterator<Item = &'static str> {
    settings().map(|setting| setting.key)
}

#[test]
fn every_config_key_has_a_setting_and_every_setting_a_key() {
    let (missing, unknown) = coverage_gaps(schema_keys(), &config_keys());
    assert!(
        missing.is_empty(),
        "config keys without a setting: {missing:?}"
    );
    assert!(
        unknown.is_empty(),
        "settings without a config key: {unknown:?}"
    );
}

#[test]
fn removing_a_setting_fails_coverage() {
    let schema = schema_keys().filter(|key| *key != "stale.stale_percent");
    let (missing, unknown) = coverage_gaps(schema, &config_keys());
    assert_eq!(missing, ["stale.stale_percent"]);
    assert_eq!(unknown, Vec::<String>::new());
}

#[test]
fn a_config_key_without_a_setting_fails_coverage() {
    let mut table = default_table().unwrap();
    table["stale"]
        .as_table_mut()
        .unwrap()
        .insert("new_knob".to_owned(), Value::Integer(1));
    let (missing, _) = coverage_gaps(schema_keys(), &leaf_keys(&table));
    assert_eq!(missing, ["stale.new_knob"]);
}

#[test]
fn a_setting_without_a_config_key_fails_coverage() {
    let schema = schema_keys().chain(["stale.gone"]);
    let (missing, unknown) = coverage_gaps(schema, &config_keys());
    assert_eq!(missing, Vec::<String>::new());
    assert_eq!(unknown, ["stale.gone"]);
}

#[test]
fn leaf_keys_treat_arrays_as_leaves() {
    let table: Table =
        toml::from_str("top = 1\n[a]\nlist = [1]\nregions = [{ x = 1 }]\n[a.b]\nc = true\n")
            .unwrap();
    let keys: BTreeSet<_> = leaf_keys(&table).into_iter().collect();
    let expected: BTreeSet<_> = ["top", "a.list", "a.regions", "a.b.c"]
        .map(str::to_owned)
        .into();
    assert_eq!(keys, expected);
}

#[test]
fn keys_are_unique_and_inside_their_section() {
    let mut seen = BTreeSet::new();
    for section in SECTIONS {
        for setting in section.settings {
            assert!(seen.insert(setting.key), "{} listed twice", setting.key);
            let expected = if section.id.is_empty() {
                setting.name().to_owned()
            } else {
                format!("{}.{}", section.id, setting.name())
            };
            assert_eq!(setting.key, expected);
        }
    }
}

#[test]
fn sections_match_config_tables() {
    let defaults = default_table().unwrap();
    let tables: BTreeSet<&str> = defaults
        .iter()
        .filter(|(_, value)| value.is_table())
        .map(|(name, _)| name.as_str())
        .collect();
    let sections: BTreeSet<&str> = SECTIONS
        .iter()
        .map(|section| section.id)
        .filter(|id| !id.is_empty())
        .collect();
    assert_eq!(sections, tables);
    assert_eq!(
        SECTIONS[0].id, "",
        "top-level keys must come before any table"
    );
}

#[test]
fn text_is_filled_in() {
    for section in SECTIONS {
        assert!(!section.title.is_empty() && section.help.ends_with('.'));
        for setting in section.settings {
            assert!(!setting.label.is_empty(), "{}", setting.key);
            assert!(setting.help.ends_with('.'), "{}", setting.key);
            for choice in setting.control.choices().unwrap_or_default() {
                assert!(!choice.label.is_empty() && choice.help.ends_with('.'));
            }
        }
    }
}

#[test]
fn every_setting_has_a_default() {
    let defaults = default_table().unwrap();
    for setting in settings() {
        assert!(setting.default_in(&defaults).is_some(), "{}", setting.key);
    }
}

#[test]
fn only_detection_keys_reset_detection() {
    let resetting: Vec<_> = settings()
        .filter(|setting| setting.resets_detection)
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

fn with_value(key: &str, value: Value) -> Result<Config, toml::de::Error> {
    let mut table = default_table().unwrap();
    let (section, name) = key.split_once('.').unwrap();
    table[section]
        .as_table_mut()
        .unwrap()
        .insert(name.to_owned(), value);
    Value::Table(table).try_into()
}

fn backticked(text: &str) -> Vec<&str> {
    text.split('`').skip(1).step_by(2).collect()
}

#[test]
fn enum_choices_are_exactly_the_config_variants() {
    for setting in settings() {
        let Some(choices) = setting.control.choices() else {
            continue;
        };
        let err = with_value(setting.key, Value::String("bogus".to_owned())).unwrap_err();
        let message = err.message();
        let (_, expected) = message.split_once("expected").unwrap();
        let values: Vec<_> = choices.iter().map(|choice| choice.value).collect();
        assert_eq!(backticked(expected), values, "{}", setting.key);

        let defaults = default_table().unwrap();
        let default = setting.default_in(&defaults).unwrap().as_str().unwrap();
        assert!(values.contains(&default), "{}", setting.key);
    }
}

fn issue_keys_for(key: &str, value: Value) -> Vec<String> {
    let config = with_value(key, value).unwrap();
    config
        .validate()
        .err()
        .unwrap_or_default()
        .into_iter()
        .map(|issue| issue.key)
        .filter(|issue_key| issue_key.starts_with(key))
        .collect()
}

fn scalar_or_list(control: &Control, value: u32) -> Value {
    let int = Value::Integer(i64::from(value));
    match control {
        Control::GridSize { .. } => Value::Array(vec![int.clone(), int]),
        Control::IntList { .. } => Value::Array(vec![int]),
        _ => int,
    }
}

#[test]
fn values_outside_the_bounds_fail_validation_at_the_key() {
    for setting in settings() {
        let Some(bounds) = setting.control.bounds() else {
            continue;
        };
        let mut outside = Vec::new();
        if let Some(below) = bounds.min.checked_sub(1) {
            outside.push(below);
        }
        if let Some(max) = bounds.upper() {
            outside.push(max + 1);
        }
        for value in outside {
            let keys = issue_keys_for(setting.key, scalar_or_list(&setting.control, value));
            assert!(!keys.is_empty(), "{} accepted {value}", setting.key);
        }
    }
}

/// Keys whose lower limit depends on another key, so the schema can only
/// give the widest range.
const CROSS_FIELD: [&str; 2] = ["safety.ceiling_minutes", "prompt.custom_max_minutes"];

#[test]
fn values_at_the_bounds_pass_validation_at_the_key() {
    for setting in settings() {
        let Some(bounds) = setting.control.bounds() else {
            continue;
        };
        if CROSS_FIELD.contains(&setting.key) {
            assert_eq!(bounds, ANY, "{}", setting.key);
            continue;
        }
        let mut inside = vec![bounds.min];
        inside.extend(bounds.upper());
        for value in inside {
            let keys = issue_keys_for(setting.key, scalar_or_list(&setting.control, value));
            assert!(
                keys.is_empty(),
                "{} rejected {value}: {keys:?}",
                setting.key
            );
        }
    }
}

#[test]
fn find_maps_indexed_keys_to_their_list() {
    assert_eq!(
        find("stale.stale_percent").unwrap().label,
        "Stale threshold"
    );
    assert_eq!(
        find("stale.ignore_regions[0].w").unwrap().key,
        "stale.ignore_regions"
    );
    assert_eq!(find("version").unwrap().control, Control::ReadOnly);
    assert!(find("stale.bogus").is_none());
}

#[test]
fn lookup_walks_dotted_paths() {
    let defaults = default_table().unwrap();
    assert_eq!(lookup(&defaults, "version"), Some(&Value::Integer(1)));
    assert_eq!(
        lookup(&defaults, "stale.stale_percent"),
        Some(&Value::Integer(70))
    );
    assert_eq!(lookup(&defaults, "stale.missing"), None);
    assert_eq!(lookup(&defaults, "version.deeper"), None);
    assert_eq!(lookup(&defaults, ""), None);
}

#[test]
fn table_of_reflects_the_config() {
    let mut config = Config::default();
    config.stale.stale_percent = 55;
    let table = table_of(&config).unwrap();
    assert_eq!(
        lookup(&table, "stale.stale_percent"),
        Some(&Value::Integer(55))
    );
}

#[test]
fn setting_name_strips_the_section() {
    assert_eq!(find("stale.block_grid").unwrap().name(), "block_grid");
    assert_eq!(find("version").unwrap().name(), "version");
}

#[test]
fn type_names_and_allowed_values() {
    let bounds = Bounds::new(1, 10);
    let cases = [
        (Control::ReadOnly, "read-only integer", None),
        (Control::Toggle, "boolean", None),
        (Control::Int { bounds, step: 1 }, "integer", Some("1 to 10")),
        (Control::Percent { bounds }, "percent", Some("1 to 10")),
        (
            Control::Duration {
                unit: TimeUnit::Hours,
                bounds: ANY,
            },
            "duration in hours",
            None,
        ),
        (Control::Text, "text", None),
        (Control::Command, "command", None),
        (Control::StringList, "list of text", None),
        (
            Control::IntList {
                unit: Some(TimeUnit::Seconds),
                bounds: POSITIVE,
            },
            "list of durations in seconds",
            Some("each at least 1"),
        ),
        (
            Control::IntList { unit: None, bounds },
            "list of integers",
            Some("each 1 to 10"),
        ),
        (
            Control::GridSize { bounds: POSITIVE },
            "grid size (cols, rows)",
            Some("each at least 1"),
        ),
        (Control::OutputPicker, "list of output names", None),
        (
            Control::GamepadPicker,
            "list of gamepad name substrings",
            None,
        ),
        (Control::PlayerPicker, "list of MPRIS player names", None),
        (Control::RegionEditor, "list of regions", None),
    ];
    for (control, name, allowed) in cases {
        assert_eq!(control.type_name(), name);
        assert_eq!(control.allowed().as_deref(), allowed, "{name}");
    }
}

#[test]
fn enum_allowed_lists_values() {
    let control = find("capture.backend").unwrap().control;
    assert_eq!(control.type_name(), "choice");
    assert_eq!(control.allowed().as_deref(), Some("auto | kwin | portal"));
    assert_eq!(control.bounds(), None);
}

#[test]
fn time_unit_names() {
    assert_eq!(TimeUnit::Seconds.name(), "seconds");
    assert_eq!(TimeUnit::Minutes.name(), "minutes");
    assert_eq!(TimeUnit::Hours.name(), "hours");
}

#[test]
fn constructors_work_at_runtime() {
    let bounds = Bounds::at_least(3);
    let setting = Setting::new("a.b", "B", Control::Percent { bounds }, "Help.");
    assert!(!setting.resets_detection);
    let resetting = setting.resetting_detection();
    assert!(resetting.resets_detection);
    assert_eq!(resetting.control.bounds(), Some(Bounds::new(3, u32::MAX)));
    assert_eq!(resetting.name(), "b");
}
