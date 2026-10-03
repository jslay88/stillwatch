use std::collections::BTreeMap;

use stillwatch_core::config::Config;

use super::super::assemble;
use super::{OLED_TV_BLANK_HOOK, OLED_TV_RESUME_HOOK, PresetDraft, changes, owned_keys, write};
use crate::edit_msg::PresetKind;

fn values() -> BTreeMap<String, super::super::values::FieldValue> {
    assemble::fields_of(&Config::default()).unwrap()
}

fn set_text(
    values: &mut BTreeMap<String, super::super::values::FieldValue>,
    key: &str,
    text: &str,
) {
    *values.get_mut(key).unwrap() = super::super::values::FieldValue::Text(text.to_owned());
}

fn set_bool(
    values: &mut BTreeMap<String, super::super::values::FieldValue>,
    key: &str,
    value: bool,
) {
    *values.get_mut(key).unwrap() = super::super::values::FieldValue::Bool(value);
}

fn keys_of(changes: &[super::KeyChange]) -> Vec<&str> {
    changes.iter().map(|change| change.key).collect()
}

#[test]
fn each_preset_diffs_only_its_keys() {
    let mut monitor = values();
    set_text(&mut monitor, "action.blank_method", "overlay");
    set_bool(&mut monitor, "action.reblank_on_wake", false);
    set_text(&mut monitor, "action.reblank_fallback", "none");
    let diff = changes(&monitor, &PresetDraft::new(PresetKind::OledMonitor));
    assert_eq!(keys_of(&diff), owned_keys(PresetKind::OledMonitor).to_vec());
    assert_eq!(diff[0].before, "overlay");
    assert_eq!(diff[0].after, "dpms");
    assert_eq!(diff[1].after, "true");
    assert_eq!(diff[2].after, "overlay");

    let mut overlay = values();
    set_text(&mut overlay, "action.blank_method", "ddc_standby");
    let diff = changes(&overlay, &PresetDraft::new(PresetKind::OledTvOverlay));
    assert_eq!(keys_of(&diff), vec!["action.blank_method"]);
    assert_eq!(diff[0].after, "overlay");

    let mut hooks = values();
    set_text(&mut hooks, "action.blank_method", "overlay");
    let diff = changes(&hooks, &PresetDraft::new(PresetKind::OledTvHooks));
    assert_eq!(keys_of(&diff), owned_keys(PresetKind::OledTvHooks).to_vec());
    assert_eq!(diff[1].after, OLED_TV_BLANK_HOOK);
    assert_eq!(diff[2].after, OLED_TV_RESUME_HOOK);

    let mut mixed = values();
    set_text(&mut mixed, "action.outputs", "all");
    let mut draft = PresetDraft::new(PresetKind::Mixed);
    draft.outputs = vec!["HDMI-A-1".into(), "DP-1".into()];
    let diff = changes(&mixed, &draft);
    assert_eq!(keys_of(&diff), owned_keys(PresetKind::Mixed).to_vec());
    assert_eq!(diff[0].after, "HDMI-A-1, DP-1");
    assert_eq!(diff[1].before, "all");
    assert_eq!(diff[1].after, "monitored");

    assert_eq!(
        changes(&values(), &PresetDraft::new(PresetKind::Custom)),
        Vec::new()
    );
}

#[test]
fn writing_a_preset_leaves_every_other_key_alone() {
    let original = values();
    for kind in [
        PresetKind::OledMonitor,
        PresetKind::OledTvOverlay,
        PresetKind::OledTvHooks,
        PresetKind::Mixed,
        PresetKind::Custom,
    ] {
        let mut next = original.clone();
        let mut draft = PresetDraft::new(kind);
        if kind == PresetKind::Mixed {
            draft.outputs = vec!["HDMI-A-1".into()];
        }
        let _ = write(&mut next, &draft);
        let owned = owned_keys(kind);
        for (key, value) in &original {
            if !owned.contains(&key.as_str()) {
                assert_eq!(next.get(key), Some(value), "{kind:?} touched {key}");
            }
        }
    }
}

#[test]
fn a_preset_that_already_matches_changes_nothing() {
    let mut values = values();
    let draft = PresetDraft::new(PresetKind::OledMonitor);
    assert_eq!(changes(&values, &draft), Vec::new());
    assert!(!write(&mut values, &draft));
}
