use std::path::Path;

use stillwatch_core::config::{MigrationNote, StaleRequire, ValidationIssue};
use stillwatch_core::history::HistoryKind;
use stillwatch_core::mocks::ScriptedDetector;
use tempfile::TempDir;

use super::*;

const GOOD: &str = "[stale]\nstale_percent = 50\n";
const BAD: &str = "[stale]\nstale_percent = 0\n";

fn dir() -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    (dir, path)
}

fn write(path: &Path, contents: &str) {
    std::fs::write(path, contents).unwrap();
}

fn applied(outcome: ReloadOutcome) -> Applied {
    match outcome {
        ReloadOutcome::Applied(applied) => *applied,
        other => panic!("expected applied, got {other:?}"),
    }
}

fn rejected(outcome: ReloadOutcome) -> ConfigError {
    match outcome {
        ReloadOutcome::Rejected(error) => error,
        other => panic!("expected rejected, got {other:?}"),
    }
}

fn keys(applied: &Applied) -> Vec<&'static str> {
    applied.changes.keys().collect()
}

#[test]
fn no_file_runs_on_defaults() {
    let (_dir, path) = dir();
    let (mut reloader, loaded) = Reloader::load(path.clone()).unwrap();
    assert_eq!(loaded, defaults());
    assert_eq!(reloader.config(), &Config::default());
    assert_eq!(reloader.path(), path);
    assert_eq!(reloader.errors(), [] as [String; 0]);

    assert!(matches!(
        reloader.reload(ReloadTrigger::FileChanged),
        ReloadOutcome::Unchanged
    ));
    let forced = applied(reloader.reload(ReloadTrigger::Hangup));
    assert!(forced.changes.is_empty());
    assert_eq!(forced.loaded.config, Config::default());
}

#[test]
fn a_bad_file_at_start_is_an_error() {
    let (_dir, path) = dir();
    write(&path, BAD);
    let error = Reloader::load(path).unwrap_err();
    assert!(matches!(error, ConfigError::Invalid(_)), "{error:?}");
}

#[test]
fn an_unreadable_file_at_start_is_an_error() {
    let (dir, _) = dir();
    let error = Reloader::load(dir.path().to_path_buf()).unwrap_err();
    assert!(matches!(error, ConfigError::Io { .. }), "{error:?}");
}

#[test]
fn same_contents_are_skipped_unless_forced() {
    let (_dir, path) = dir();
    write(&path, GOOD);
    let (mut reloader, loaded) = Reloader::load(path.clone()).unwrap();
    assert_eq!(loaded.config.stale.stale_percent, 50);

    assert!(matches!(
        reloader.reload(ReloadTrigger::FileChanged),
        ReloadOutcome::Unchanged
    ));
    for trigger in [ReloadTrigger::Hangup, ReloadTrigger::Requested] {
        let forced = applied(reloader.reload(trigger));
        assert!(forced.changes.is_empty(), "{trigger:?}");
    }
}

#[test]
fn an_edit_applies_with_its_changes() {
    let (_dir, path) = dir();
    write(&path, GOOD);
    let (mut reloader, _) = Reloader::load(path.clone()).unwrap();
    write(&path, "[stale]\nstale_percent = 50\nrequire = \"any\"\n");
    let applied = applied(reloader.reload(ReloadTrigger::FileChanged));
    assert_eq!(keys(&applied), ["stale.require"]);
    assert!(!applied.changes.resets_detection());
    assert_eq!(reloader.config().stale.require, StaleRequire::Any);
    assert_eq!(reloader.config(), &applied.loaded.config);
}

#[test]
fn an_invalid_edit_keeps_the_last_good_config() {
    let (_dir, path) = dir();
    write(&path, GOOD);
    let (mut reloader, _) = Reloader::load(path.clone()).unwrap();
    write(&path, BAD);

    let outcome = reloader.reload(ReloadTrigger::FileChanged);
    let report = outcome.report().unwrap();
    assert!(!report.ok);
    assert_eq!(report.errors.len(), 1);
    assert!(
        report.errors[0].starts_with("stale.stale_percent: "),
        "{report:?}"
    );
    assert!(matches!(rejected(outcome), ConfigError::Invalid(_)));
    assert_eq!(reloader.config().stale.stale_percent, 50);
    assert_eq!(reloader.errors(), report.errors);

    assert!(matches!(
        reloader.reload(ReloadTrigger::FileChanged),
        ReloadOutcome::Unchanged
    ));
    rejected(reloader.reload(ReloadTrigger::Requested));

    write(&path, GOOD);
    let fixed = applied(reloader.reload(ReloadTrigger::FileChanged));
    assert!(fixed.changes.is_empty());
    assert_eq!(reloader.errors(), [] as [String; 0]);
}

#[test]
fn a_deleted_file_keeps_the_last_good_config_until_it_returns() {
    let (_dir, path) = dir();
    write(&path, GOOD);
    let (mut reloader, _) = Reloader::load(path.clone()).unwrap();
    std::fs::remove_file(&path).unwrap();

    let outcome = reloader.reload(ReloadTrigger::FileChanged);
    let report = outcome.report().unwrap();
    assert!(report.errors[0].starts_with("config file not found: "));
    assert!(matches!(rejected(outcome), ConfigError::NotFound { .. }));
    assert_eq!(reloader.config().stale.stale_percent, 50);
    assert!(matches!(
        reloader.reload(ReloadTrigger::FileChanged),
        ReloadOutcome::Unchanged
    ));

    write(&path, GOOD);
    assert!(
        applied(reloader.reload(ReloadTrigger::FileChanged))
            .changes
            .is_empty()
    );
    assert_eq!(reloader.errors(), [] as [String; 0]);
}

#[test]
fn a_file_that_appears_then_goes_away_is_kept() {
    let (_dir, path) = dir();
    let (mut reloader, _) = Reloader::load(path.clone()).unwrap();
    write(&path, GOOD);
    let appeared = applied(reloader.reload(ReloadTrigger::FileChanged));
    assert_eq!(keys(&appeared), ["stale.stale_percent"]);

    std::fs::remove_file(&path).unwrap();
    rejected(reloader.reload(ReloadTrigger::FileChanged));
    assert_eq!(reloader.config().stale.stale_percent, 50);
}

#[test]
fn read_errors_are_retried_every_time() {
    let (dir, _) = dir();
    std::fs::write(dir.path().join("config.toml"), GOOD).unwrap();
    let (mut reloader, _) = Reloader::load(dir.path().join("config.toml")).unwrap();
    reloader.path = dir.path().to_path_buf();
    for _ in 0..2 {
        let error = rejected(reloader.reload(ReloadTrigger::FileChanged));
        assert!(matches!(error, ConfigError::Io { .. }));
    }
}

#[test]
fn reports_match_outcomes() {
    assert_eq!(ReloadOutcome::Unchanged.report(), None);
    let applied = ReloadOutcome::Applied(Box::new(Applied {
        loaded: defaults(),
        changes: ConfigChanges::default(),
    }));
    assert_eq!(applied.report(), Some(ReloadReport::applied()));
}

#[test]
fn messages_are_one_per_problem() {
    let issue = |key: &str| ValidationIssue {
        key: key.into(),
        message: "out of range".into(),
    };
    let invalid = ConfigError::Invalid(vec![issue("a.b"), issue("c.d")]);
    assert_eq!(
        error_messages(&invalid),
        ["a.b: out of range", "c.d: out of range"]
    );
    let syntax = Config::from_toml_str("[stale").unwrap_err();
    assert_eq!(error_messages(&syntax), [syntax.to_string()]);
}

fn machine() -> StateMachine {
    let (machine, _) = StateMachine::new(
        &Config::default(),
        Box::new(ScriptedDetector::new()),
        Instant::now(),
    );
    machine
}

fn recorded(commands: &[Command]) -> Vec<HistoryKind> {
    commands
        .iter()
        .filter_map(|command| match command {
            Command::Record(entry) => Some(entry.kind),
            _ => None,
        })
        .collect()
}

#[test]
fn outcomes_reach_the_machine_history() {
    let mut machine = machine();
    let (now, wall) = (Instant::now(), Timestamp::now());
    assert_eq!(
        ReloadOutcome::Unchanged.update_machine(&mut machine, now, wall),
        []
    );

    let mut config = Config::default();
    config.stale.stale_percent = 50;
    let migrated = ReloadOutcome::Applied(Box::new(Applied {
        loaded: LoadOutcome {
            config: config.clone(),
            migrated_from: Some(0),
            notes: vec![MigrationNote {
                from: 0,
                to: 1,
                message: "renamed a key".into(),
            }],
        },
        changes: ConfigChanges::default(),
    }));
    let commands = migrated.update_machine(&mut machine, now, wall);
    assert_eq!(
        recorded(&commands),
        [HistoryKind::ConfigReload, HistoryKind::Migration]
    );
    assert_eq!(machine.config(), &config);

    let failed = ReloadOutcome::Rejected(Config::from_toml_str("[stale").unwrap_err());
    let commands = failed.update_machine(&mut machine, now, wall);
    assert_eq!(recorded(&commands), [HistoryKind::ConfigReloadFailed]);
    assert_eq!(machine.config(), &config);
}

#[test]
fn migrations_are_logged_without_touching_the_file() {
    let (_dir, path) = dir();
    write(&path, GOOD);
    log_migration(&LoadOutcome {
        migrated_from: Some(0),
        ..defaults()
    });
    assert_eq!(std::fs::read_to_string(&path).unwrap(), GOOD);
}
