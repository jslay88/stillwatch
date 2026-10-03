//! History entries beyond transitions: prompts, blanks, reloads, migrations.

use std::path::PathBuf;
use std::time::Duration;

use super::{effects, record_kinds, records};
use crate::command::BlankMethod;
use crate::config::{Config, ConfigError, LoadOutcome, ValidationIssue};
use crate::event::{ControlCommand, Event};
use crate::history::{HistoryKind, PromptAnswer};
use crate::mocks::Harness;
use crate::prompt::PromptOutcome;
use crate::state::State;

#[test]
fn snooze_path_records_prompt_answer_and_snooze() {
    let mut h = Harness::new();
    h.to_prompting();
    h.answer(PromptOutcome::Snooze(Duration::from_hours(1)));
    h.send(ControlCommand::Snooze(Duration::from_mins(15)));
    let kinds = record_kinds(h.log());
    assert_eq!(
        kinds,
        [
            HistoryKind::Transition,
            HistoryKind::Transition,
            HistoryKind::Prompt,
            HistoryKind::PromptAnswered,
            HistoryKind::Transition,
            HistoryKind::Snooze,
        ]
    );
    let entries = records(h.log());
    assert_eq!(entries[3].snooze_seconds, Some(3600));
    assert_eq!(entries[4].snooze_seconds, Some(3600));
    assert_eq!(entries[5].snooze_seconds, Some(900));
}

#[test]
fn cancel_and_prompter_timeout_are_recorded_before_the_transition() {
    for (outcome, answer, to) in [
        (PromptOutcome::Cancel, PromptAnswer::Cancel, State::Active),
        (PromptOutcome::Timeout, PromptAnswer::Timeout, State::Acting),
    ] {
        let mut h = Harness::new();
        h.to_prompting();
        let entries = records(&h.answer(outcome));
        assert_eq!(entries[0].kind, HistoryKind::PromptAnswered);
        assert_eq!(entries[0].answer, Some(answer));
        assert_eq!(entries[0].snooze_seconds, None);
        assert_eq!(entries[1].to, Some(to));
    }
}

#[test]
fn input_during_a_prompt_is_a_transition_not_an_answer() {
    let mut h = Harness::new();
    h.to_prompting();
    assert_eq!(record_kinds(&h.input()), [HistoryKind::Transition]);
}

#[test]
fn prompt_entry_carries_the_detection_and_context() {
    let mut h = Harness::new();
    h.send(Event::Media {
        playing: vec!["mpv".into()],
    });
    let commands = h.to_prompting();
    let prompt = records(&commands)
        .into_iter()
        .find(|entry| entry.kind == HistoryKind::Prompt)
        .unwrap();
    assert!(prompt.detection.is_some_and(|stats| stats.stale));
    assert!(prompt.context.media_playing);
    assert_eq!((prompt.from, prompt.to), (None, None));
}

#[test]
fn blank_entry_names_the_method_once_per_blank_command() {
    let mut config = Config::default();
    config.action.blank_method = BlankMethod::DdcStandby;
    let mut h = Harness::with_config(&config);
    h.to_blanked();
    let blanks: Vec<_> = records(h.log())
        .into_iter()
        .filter(|entry| entry.kind == HistoryKind::Blank)
        .collect();
    assert_eq!(blanks.len(), 1);
    assert_eq!(blanks[0].blank_method, Some(BlankMethod::DdcStandby));
}

#[test]
fn failed_reload_records_only_the_problem_count() {
    let mut h = Harness::new();
    let issue = |key: &str| ValidationIssue {
        key: key.into(),
        message: "out of range".into(),
    };
    let invalid = ConfigError::Invalid(vec![
        issue("stale.stale_percent"),
        issue("history.max_entries"),
    ]);
    let commands = h.reload_failed(&invalid);
    assert_eq!(commands.len(), 1);
    let entry = &records(&commands)[0];
    assert_eq!(entry.kind, HistoryKind::ConfigReloadFailed);
    assert_eq!(entry.error_count, Some(2));
    let json = serde_json::to_string(entry).unwrap();
    assert!(!json.contains("stale_percent") && !json.contains("out of range"));

    let unreadable = ConfigError::Io {
        path: PathBuf::from("/home/me/.config/stillwatch/config.toml"),
        source: std::io::Error::other("denied"),
    };
    let entry = &records(&h.reload_failed(&unreadable))[0];
    assert_eq!(entry.error_count, Some(1));
    assert!(!serde_json::to_string(entry).unwrap().contains("/home"));
    assert_eq!(h.machine().config(), &Config::default());
    assert_eq!(h.state(), State::Active);
}

#[test]
fn migration_is_recorded_only_when_it_happened() {
    let mut h = Harness::new();
    let current = LoadOutcome {
        config: Config::default(),
        migrated_from: None,
        notes: vec![],
    };
    assert_eq!(h.loaded(&current), vec![]);

    let migrated = LoadOutcome {
        config: Config {
            version: 3,
            ..Config::default()
        },
        migrated_from: Some(1),
        notes: vec![],
    };
    let commands = h.loaded(&migrated);
    assert_eq!(effects(&commands), vec![]);
    let entry = &records(&commands)[0];
    assert_eq!(entry.kind, HistoryKind::Migration);
    assert_eq!((entry.from_version, entry.to_version), (Some(1), Some(3)));
}

#[test]
fn successful_reload_still_records_config_reload() {
    let mut h = Harness::new();
    let commands = h.apply_config(&Config::default());
    assert_eq!(record_kinds(&commands), [HistoryKind::ConfigReload]);
}
