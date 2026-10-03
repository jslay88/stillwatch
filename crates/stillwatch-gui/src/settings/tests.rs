//! Settings form: controls, validation, dirty state, the sync banner, and saves.

use std::fs;
use std::os::unix::fs::symlink;

use stillwatch_core::config::{CaptureBackend, Config, IgnoreRegion, LoadOutcome};
use stillwatch_core::schema::{self, Control, Setting};
use stillwatch_core::state::State;
use stillwatch_ipc::status::StatusPayload;

use crate::edit_msg::{FieldChange, RestoreScope, SettingsMsg};
use crate::model::update;
use crate::shell::{DaemonCall, DaemonEvent, Link, Message, Shell, Snapshot};

use super::controls;
use super::document;
use super::editor::{Banner, Editor};
use super::values::{self, FieldValue};
use super::view;
use super::{assemble, handle};

fn text_of(editor: &Editor, key: &str) -> String {
    match editor.field(key) {
        Some(FieldValue::Text(text)) => text.clone(),
        other => panic!("{key} is {other:?}"),
    }
}

fn bool_of(editor: &Editor, key: &str) -> bool {
    match editor.field(key) {
        Some(FieldValue::Bool(value)) => *value,
        other => panic!("{key} is {other:?}"),
    }
}

fn edit(editor: &mut Editor, change: FieldChange) {
    let _ = handle(editor, None, SettingsMsg::Edit(change));
}

fn shell_at(path: &std::path::Path) -> Shell {
    let mut shell = Shell::new(vec![15, 60]);
    shell.config_path = Some(path.to_path_buf());
    shell.editor = Editor::load(path).unwrap();
    shell
}

#[test]
fn every_schema_key_builds_a_control() {
    let editor = Editor::pristine();
    assert!(editor.issues().is_empty(), "{:?}", editor.issues());
    let mut seen = Vec::new();
    for setting in schema::settings() {
        let field = editor
            .field(setting.key)
            .unwrap_or_else(|| panic!("no value for {}", setting.key));
        let _ = controls::widget(setting, field, editor.draft(setting.key), &[]);
        seen.push(setting.key);
    }
    assert_eq!(seen.len(), schema::settings().count());

    let text = Setting::new("extra.note", "Note", Control::Text, "Free text.");
    let _ = controls::widget(&text, &FieldValue::Text("hi".into()), "", &[]);
    let tags = Setting::new("extra.tags", "Tags", Control::StringList, "A list.");
    let _ = controls::widget(&tags, &FieldValue::List(vec!["a".into()]), "b", &[]);

    let _ = view::page(&editor, &[]);
}

#[test]
fn field_values_round_trip_through_the_core_config() {
    let mut config = Config::default();
    config.idle.input_idle_minutes = 12;
    config.activity.gamepad = false;
    config.activity.gamepad_ignore_devices = vec!["pad".to_owned()];
    config.capture.backend = CaptureBackend::Portal;
    config.stale.block_grid = [8, 4];
    config.stale.monitored_outputs = vec!["HDMI-A-1".to_owned()];
    config.stale.ignore_regions = vec![IgnoreRegion {
        output: "HDMI-A-1".to_owned(),
        x: 1,
        y: 2,
        w: 3,
        h: 4,
    }];
    config.prompt.snooze_presets_minutes = vec![5, 10];
    config.action.command = "echo hi".to_owned();

    let fields = assemble::fields_of(&config).unwrap();
    assert_eq!(assemble::config(&fields).unwrap(), config);
}

#[test]
fn validation_errors_land_on_the_fields() {
    let mut editor = Editor::pristine();
    edit(
        &mut editor,
        FieldChange::Text {
            key: "stale.stale_percent".to_owned(),
            value: "0".to_owned(),
        },
    );
    assert!(!editor.can_save());
    assert!(
        editor
            .issues()
            .iter()
            .any(|issue| issue.key == "stale.stale_percent")
    );

    edit(
        &mut editor,
        FieldChange::Text {
            key: "stale.stale_percent".to_owned(),
            value: "nope".to_owned(),
        },
    );
    assert!(
        editor
            .issues()
            .iter()
            .any(|issue| issue.key == "stale.stale_percent")
    );

    edit(
        &mut editor,
        FieldChange::RegionPush {
            key: "stale.ignore_regions".to_owned(),
        },
    );
    edit(
        &mut editor,
        FieldChange::Region {
            key: "stale.ignore_regions".to_owned(),
            index: 0,
            field: crate::edit_msg::RegionPart::W,
            value: "0".to_owned(),
        },
    );
    let messages = values::field_messages(&editor.issues(), "stale.ignore_regions");
    assert!(
        messages.iter().any(|message| message.contains("[0].w")),
        "{messages:?}"
    );

    let lines = vec![
        "stale.stale_percent: must be between 1 and 100, got 0".to_owned(),
        "config file not found: /tmp/nope".to_owned(),
    ];
    let (keyed, other) = values::external_issues(&lines);
    assert_eq!(
        values::field_messages(&keyed, "stale.stale_percent"),
        vec!["must be between 1 and 100, got 0".to_owned()]
    );
    assert_eq!(other, vec!["config file not found: /tmp/nope".to_owned()]);
}

#[test]
fn edits_are_dirty_until_they_match_the_baseline() {
    let mut editor = Editor::pristine();
    assert!(!editor.is_dirty());
    edit(
        &mut editor,
        FieldChange::Bool {
            key: "activity.gamepad".to_owned(),
            value: false,
        },
    );
    assert!(editor.is_dirty());
    assert!(editor.can_save());
    edit(
        &mut editor,
        FieldChange::Bool {
            key: "activity.gamepad".to_owned(),
            value: true,
        },
    );
    assert!(!editor.is_dirty());
    assert!(!editor.can_save());
}

#[test]
fn restore_waits_for_confirmation_and_is_scoped() {
    let mut editor = Editor::pristine();
    edit(
        &mut editor,
        FieldChange::Text {
            key: "idle.input_idle_minutes".to_owned(),
            value: "20".to_owned(),
        },
    );
    edit(
        &mut editor,
        FieldChange::Bool {
            key: "activity.gamepad".to_owned(),
            value: false,
        },
    );
    let _ = handle(
        &mut editor,
        None,
        SettingsMsg::AskRestore(RestoreScope::Section("idle".to_owned())),
    );
    assert!(editor.pending().is_some());
    assert_eq!(text_of(&editor, "idle.input_idle_minutes"), "20");
    assert!(editor.confirm_prompt().unwrap().contains("Idle"));
    let _ = view::page(&editor, &[]);

    let _ = handle(&mut editor, None, SettingsMsg::CancelRestore);
    assert!(editor.pending().is_none());
    assert_eq!(text_of(&editor, "idle.input_idle_minutes"), "20");

    let _ = handle(
        &mut editor,
        None,
        SettingsMsg::AskRestore(RestoreScope::Section("idle".to_owned())),
    );
    let _ = handle(&mut editor, None, SettingsMsg::ConfirmRestore);
    assert_eq!(text_of(&editor, "idle.input_idle_minutes"), "10");
    assert!(!bool_of(&editor, "activity.gamepad"));

    let _ = handle(
        &mut editor,
        None,
        SettingsMsg::AskRestore(RestoreScope::All),
    );
    assert!(editor.confirm_prompt().unwrap().contains("every setting"));
    let _ = handle(&mut editor, None, SettingsMsg::ConfirmRestore);
    assert!(bool_of(&editor, "activity.gamepad"));
    assert!(!editor.is_dirty());
}

#[test]
fn config_changed_refreshes_when_clean_and_banners_when_dirty() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let original = "# keep\nversion = 1\n\n[idle]\n# minutes\ninput_idle_minutes = 10\n";
    fs::write(&path, original).unwrap();
    let mut shell = shell_at(&path);
    assert!(!shell.editor.is_dirty());

    let changed = "# keep\nversion = 1\n\n[idle]\n# minutes\ninput_idle_minutes = 12\n";
    fs::write(&path, changed).unwrap();
    assert_eq!(
        update(
            &mut shell,
            Message::Daemon(DaemonEvent::Config {
                ok: true,
                errors: Vec::new(),
            }),
        ),
        Vec::new()
    );
    assert!(shell.editor.banner().is_none());
    assert_eq!(text_of(&shell.editor, "idle.input_idle_minutes"), "12");

    assert_eq!(
        update(
            &mut shell,
            Message::Settings(SettingsMsg::Edit(FieldChange::Text {
                key: "idle.input_idle_minutes".to_owned(),
                value: "9".to_owned(),
            })),
        ),
        Vec::new()
    );
    fs::write(&path, original).unwrap();
    assert_eq!(
        update(
            &mut shell,
            Message::Daemon(DaemonEvent::Config {
                ok: true,
                errors: Vec::new(),
            }),
        ),
        Vec::new()
    );
    assert_eq!(shell.editor.banner(), Some(Banner::DiskChanged));
    assert_eq!(text_of(&shell.editor, "idle.input_idle_minutes"), "9");
    let _ = view::page(&shell.editor, &shell.config_errors);

    assert_eq!(
        update(&mut shell, Message::Settings(SettingsMsg::KeepEdits)),
        Vec::new()
    );
    assert!(shell.editor.banner().is_none());
    assert_eq!(text_of(&shell.editor, "idle.input_idle_minutes"), "9");

    assert_eq!(
        update(&mut shell, Message::Settings(SettingsMsg::ReloadDisk)),
        Vec::new()
    );
    assert_eq!(text_of(&shell.editor, "idle.input_idle_minutes"), "10");
    assert!(!shell.editor.is_dirty());
}

#[test]
fn status_reload_errors_are_kept_on_the_shell() {
    let mut shell = Shell::new(vec![15]);
    let status = StatusPayload {
        config_errors: vec!["stale.stale_percent: must be between 1 and 100, got 0".to_owned()],
        ..StatusPayload::new(State::Monitoring)
    };
    assert_eq!(
        update(
            &mut shell,
            Message::Daemon(DaemonEvent::Snapshot(Snapshot::from_status(&status))),
        ),
        Vec::new()
    );
    assert_eq!(shell.config_errors, status.config_errors);
    assert_eq!(shell.config_ok, Some(false));
    let (keyed, _) = values::external_issues(&shell.config_errors);
    assert_eq!(
        values::field_messages(&keyed, "stale.stale_percent"),
        vec!["must be between 1 and 100, got 0".to_owned()]
    );
}

#[test]
fn save_writes_atomically_and_reloads_only_when_the_daemon_is_up() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "version = 1\n").unwrap();
    let mut shell = shell_at(&path);
    assert_eq!(
        update(
            &mut shell,
            Message::Settings(SettingsMsg::Edit(FieldChange::Text {
                key: "idle.input_idle_minutes".to_owned(),
                value: "14".to_owned(),
            })),
        ),
        Vec::new()
    );
    assert_eq!(
        update(&mut shell, Message::Settings(SettingsMsg::Save)),
        vec![]
    );
    let written = fs::read_to_string(&path).unwrap();
    assert!(written.contains("input_idle_minutes = 14"), "{written}");
    assert!(fs::read_dir(dir.path()).unwrap().all(|entry| {
        let name = entry.unwrap().file_name();
        let name = name.to_string_lossy();
        !name.contains(".tmp") && !name.starts_with(".stillwatch-")
    }));
    assert!(!shell.editor.is_dirty());

    shell.link = Link::Up(Snapshot {
        state: State::Active,
        snooze_remaining_seconds: None,
        config_errors: Vec::new(),
    });
    assert_eq!(
        update(
            &mut shell,
            Message::Settings(SettingsMsg::Edit(FieldChange::Text {
                key: "idle.input_idle_minutes".to_owned(),
                value: "0".to_owned(),
            })),
        ),
        Vec::new()
    );
    assert_eq!(
        update(&mut shell, Message::Settings(SettingsMsg::Save)),
        vec![]
    );
    assert!(
        fs::read_to_string(&path)
            .unwrap()
            .contains("input_idle_minutes = 14")
    );

    assert_eq!(
        update(
            &mut shell,
            Message::Settings(SettingsMsg::Edit(FieldChange::Text {
                key: "idle.input_idle_minutes".to_owned(),
                value: "16".to_owned(),
            })),
        ),
        Vec::new()
    );
    assert_eq!(
        update(&mut shell, Message::Settings(SettingsMsg::Save)),
        vec![DaemonCall::Reload]
    );
    assert!(
        fs::read_to_string(&path)
            .unwrap()
            .contains("input_idle_minutes = 16")
    );
}

#[test]
fn save_follows_a_symlink_and_keeps_comments() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real.toml");
    let link = dir.path().join("config.toml");
    fs::write(
        &real,
        "# keep this\nversion = 1\n\n[idle]\n# minutes\ninput_idle_minutes = 10\n",
    )
    .unwrap();
    symlink(&real, &link).unwrap();
    let mut shell = shell_at(&link);
    assert_eq!(
        update(
            &mut shell,
            Message::Settings(SettingsMsg::Edit(FieldChange::Text {
                key: "idle.input_idle_minutes".to_owned(),
                value: "18".to_owned(),
            })),
        ),
        Vec::new()
    );
    assert_eq!(
        update(&mut shell, Message::Settings(SettingsMsg::Save)),
        Vec::new()
    );
    assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
    let written = fs::read_to_string(&real).unwrap();
    assert!(written.contains("# keep this"), "{written}");
    assert!(written.contains("# minutes"), "{written}");
    assert!(written.contains("input_idle_minutes = 18"), "{written}");
}

#[test]
fn a_migrated_config_writes_the_new_version() {
    let text = "# keep this\nversion = 0\n";
    let outcome = LoadOutcome {
        config: Config::default(),
        migrated_from: Some(0),
        notes: Vec::new(),
    };
    let mut editor = Editor::from_loaded(text, &outcome).unwrap();
    assert!(editor.is_dirty());
    assert!(editor.can_save());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, text).unwrap();
    let outcome = handle(&mut editor, Some(&path), SettingsMsg::Save);
    assert!(outcome.reload);
    let written = fs::read_to_string(&path).unwrap();
    assert!(written.contains("# keep this"), "{written}");
    assert!(written.contains("version = 1"), "{written}");
    assert!(!written.contains("version = 0"), "{written}");
    assert!(!editor.is_dirty());
}

#[test]
fn missing_keys_are_inserted_in_schema_order_without_moving_existing_ones() {
    let document = "\
# header
version = 1

[stale]
# percent comment
stale_percent = 70
check_interval_seconds = 60
";
    let written = document::apply(document, &Config::default()).unwrap();
    assert!(written.contains("# header"), "{written}");
    assert!(
        written.contains("# percent comment\nstale_percent = 70"),
        "{written}"
    );
    let mut doc = written.parse::<toml_edit::DocumentMut>().unwrap();
    let stale = doc
        .get_mut("stale")
        .and_then(toml_edit::Item::as_table_mut)
        .unwrap();
    let names: Vec<_> = stale.iter().map(|(key, _)| key.to_owned()).collect();
    let stale_at = names.iter().position(|key| key == "stale_percent").unwrap();
    let check_at = names
        .iter()
        .position(|key| key == "check_interval_seconds")
        .unwrap();
    let persist_at = names
        .iter()
        .position(|key| key == "persist_checks")
        .unwrap();
    assert!(stale_at < check_at, "{names:?}");
    assert!(check_at < persist_at, "{names:?}");

    let session = doc
        .get_mut("session")
        .and_then(toml_edit::Item::as_table_mut)
        .unwrap();
    let session_names: Vec<_> = session.iter().map(|(key, _)| key.to_owned()).collect();
    assert_eq!(
        session_names,
        vec!["when_locked".to_owned(), "locked_blank_seconds".to_owned()]
    );
}

#[test]
fn changing_a_value_keeps_its_comment_and_the_following_key() {
    let document = "\
# header
version = 1

[idle]
# minutes
input_idle_minutes = 10

[session]
when_locked = \"pause\"
";
    let mut config = Config::default();
    config.idle.input_idle_minutes = 11;
    config.session.when_locked = stillwatch_core::config::WhenLocked::Pause;
    let written = document::apply(document, &config).unwrap();
    assert!(written.contains("# header\nversion = 1"), "{written}");
    assert!(
        written.contains("# minutes\ninput_idle_minutes = 11"),
        "{written}"
    );
    assert!(written.contains("when_locked = \"pause\""), "{written}");
}
