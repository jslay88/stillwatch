//! History holds numbers and state names only. Every kind, with every field
//! filled in, serializes to an allowlisted set of keys.

use std::collections::BTreeSet;
use std::time::Duration;

use jiff::Timestamp;
use serde_json::Value;

use super::*;
use crate::stats::{BlockCounts, OutputStats, Threshold, ThresholdReason};

/// Every key a serialized entry may contain, at any depth.
const ALLOWED: &[&str] = &[
    "at",
    "kind",
    "from",
    "to",
    "detection",
    "outputs",
    "output",
    "persistent_percent",
    "dark_percent",
    "counted_percent",
    "stale",
    "threshold",
    "percent",
    "reason",
    "blank_method",
    "snooze_seconds",
    "reblank_attempt",
    "answer",
    "error_count",
    "from_version",
    "to_version",
    "media_playing",
    "gamepad_active",
    "locked",
];

/// Words that would mean pixels or media metadata leaked into history.
const FORBIDDEN: &[&str] = &[
    "luma", "pixel", "grid", "frame", "title", "player", "track", "artist", "album", "window",
    "message", "path",
];

/// One of each kind. The exhaustive match makes a new kind fail to compile
/// here until it's added to this test.
fn every_kind() -> Vec<HistoryKind> {
    let kinds = [
        HistoryKind::Transition,
        HistoryKind::Prompt,
        HistoryKind::Snooze,
        HistoryKind::Blank,
        HistoryKind::Reblank,
        HistoryKind::Ceiling,
        HistoryKind::ConfigReload,
        HistoryKind::OverlayUsed,
        HistoryKind::PromptAnswered,
        HistoryKind::ConfigReloadFailed,
        HistoryKind::Migration,
    ];
    for kind in kinds {
        match kind {
            HistoryKind::Transition
            | HistoryKind::Prompt
            | HistoryKind::Snooze
            | HistoryKind::Blank
            | HistoryKind::Reblank
            | HistoryKind::Ceiling
            | HistoryKind::ConfigReload
            | HistoryKind::OverlayUsed
            | HistoryKind::PromptAnswered
            | HistoryKind::ConfigReloadFailed
            | HistoryKind::Migration => {}
        }
    }
    kinds.to_vec()
}

fn full_entry(kind: HistoryKind) -> HistoryEntry {
    let counts = BlockCounts {
        total: 256,
        counted: 200,
        persistent: 180,
        dark: 40,
        ignored: 16,
    };
    let mut entry = HistoryEntry::transition(
        Timestamp::from_second(1_790_000_000).unwrap(),
        State::Monitoring,
        State::Prompting,
    )
    .with_detection(DetectionStats {
        outputs: vec![OutputStats::from_counts("HDMI-A-1", counts, 70)],
        threshold: Threshold::new(90, ThresholdReason::Media),
        stale: true,
    })
    .with_blank_method(BlankMethod::Overlay)
    .with_snooze(Duration::from_mins(15))
    .with_reblank_attempt(2)
    .with_answer(PromptAnswer::Snooze)
    .with_error_count(3)
    .with_versions(1, 2)
    .with_context(DecisionContext {
        media_playing: true,
        gamepad_active: true,
        locked: true,
    });
    entry.kind = kind;
    entry
}

fn collect_keys(value: &Value, keys: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                keys.insert(key.clone());
                collect_keys(child, keys);
            }
        }
        Value::Array(items) => items.iter().for_each(|item| collect_keys(item, keys)),
        _ => {}
    }
}

#[test]
fn every_kind_serializes_only_allowed_keys() {
    let allowed: BTreeSet<String> = ALLOWED.iter().map(|key| (*key).to_owned()).collect();
    for kind in every_kind() {
        let value = serde_json::to_value(full_entry(kind)).unwrap();
        let mut keys = BTreeSet::new();
        collect_keys(&value, &mut keys);
        let unexpected: Vec<_> = keys.difference(&allowed).collect();
        assert!(unexpected.is_empty(), "{kind:?} leaked keys {unexpected:?}");
        assert_eq!(keys, allowed, "{kind:?} should fill every field");
    }
}

#[test]
fn no_allowed_key_names_pixels_or_metadata() {
    for key in ALLOWED {
        for word in FORBIDDEN {
            assert!(!key.contains(word), "`{key}` looks like `{word}`");
        }
    }
}

#[test]
fn strings_are_only_names_and_timestamps() {
    let value = serde_json::to_value(full_entry(HistoryKind::Transition)).unwrap();
    let mut strings = Vec::new();
    collect_strings(&value, &mut strings);
    strings.sort();
    assert_eq!(
        strings,
        [
            "2026-09-21T14:13:20Z",
            "HDMI-A-1",
            "media",
            "monitoring",
            "overlay",
            "prompting",
            "snooze",
            "transition",
        ]
    );
}

fn collect_strings(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(text) => out.push(text.clone()),
        Value::Object(map) => map.values().for_each(|child| collect_strings(child, out)),
        Value::Array(items) => items.iter().for_each(|item| collect_strings(item, out)),
        _ => {}
    }
}

#[test]
fn prompt_answers_cover_every_outcome() {
    let outcomes = [
        (
            PromptOutcome::Snooze(Duration::from_mins(1)),
            PromptAnswer::Snooze,
        ),
        (PromptOutcome::CustomRequested, PromptAnswer::Custom),
        (PromptOutcome::Cancel, PromptAnswer::Cancel),
        (PromptOutcome::Timeout, PromptAnswer::Timeout),
        (PromptOutcome::Dismissed, PromptAnswer::Dismissed),
    ];
    for (outcome, answer) in outcomes {
        assert_eq!(PromptAnswer::from(outcome), answer);
    }
    assert_eq!(
        serde_json::to_string(&PromptAnswer::Failed).unwrap(),
        r#""failed""#
    );
}
