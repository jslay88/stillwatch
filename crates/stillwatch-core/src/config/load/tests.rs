use super::*;
use crate::config::test_support::{SYNTHETIC_V0_TO_V1 as V0_TO_V1, fixture};

fn version_error(input: &str) -> String {
    match Config::from_toml_str(input) {
        Err(ConfigError::InvalidVersion { found }) => found,
        other => panic!("expected invalid version for {input:?}, got {other:?}"),
    }
}

#[test]
fn missing_version_is_current() {
    let outcome = Config::from_toml_str("[idle]\ninput_idle_minutes = 5\n").unwrap();
    assert_eq!(outcome.config.version, CURRENT_VERSION);
    assert_eq!(outcome.config.idle.input_idle_minutes, 5);
    assert_eq!(outcome.migrated_from, None);
}

#[test]
fn explicit_current_version_is_not_a_migration() {
    let outcome = Config::from_toml_str("version = 1\n").unwrap();
    assert_eq!(outcome.migrated_from, None);
    assert_eq!(outcome.notes, []);
}

#[test]
fn non_integer_or_negative_version_is_invalid() {
    assert_eq!(version_error("version = -1\n"), "-1");
    assert_eq!(version_error("version = \"1\"\n"), "\"1\"");
    assert_eq!(version_error("version = 1.5\n"), "1.5");
    assert_eq!(version_error("version = 4294967296\n"), "4294967296");
}

#[test]
fn invalid_version_message() {
    let error = Config::from_toml_str("version = \"one\"\n").unwrap_err();
    assert_eq!(
        error.to_string(),
        "invalid `version`: expected a non-negative integer, got \"one\""
    );
}

#[test]
fn newer_version_is_refused_with_clear_message() {
    let error = Config::from_toml_str(&fixture("version_too_new.toml")).unwrap_err();
    assert!(matches!(
        error,
        ConfigError::VersionTooNew {
            found: 2,
            supported: 1
        }
    ));
    assert_eq!(
        error.to_string(),
        "config version 2 is newer than this build supports (version 1); \
         upgrade Stillwatch or lower `version`"
    );
}

#[test]
fn older_fixture_is_migrated_in_memory() {
    let outcome = parse_with(&[V0_TO_V1], 1, &fixture("v0_threshold_rename.toml")).unwrap();
    let mut expected = Config::default();
    expected.stale.stale_percent = 80;
    expected.stale.persist_checks = 4;
    assert_eq!(outcome.config, expected);
    assert_eq!(outcome.migrated_from, Some(0));
    assert_eq!(
        outcome.notes,
        [MigrationNote {
            from: 0,
            to: 1,
            message: "renamed `stale.threshold_percent` to `stale.stale_percent`".to_owned(),
        }]
    );
}

#[test]
fn migrated_document_is_still_validated() {
    let input = "version = 0\n[stale]\nthreshold_percent = 0\n";
    let error = parse_with(&[V0_TO_V1], 1, input).unwrap_err();
    let ConfigError::Invalid(issues) = error else {
        panic!("expected validation failure, got {error:?}");
    };
    assert_eq!(issues[0].key, "stale.stale_percent");
}

#[test]
fn version_without_migration_path_is_too_old() {
    let error = Config::from_toml_str(&fixture("v0_threshold_rename.toml")).unwrap_err();
    assert!(matches!(
        error,
        ConfigError::VersionTooOld {
            found: 0,
            missing: 0
        }
    ));
}

#[test]
fn malformed_toml_is_a_syntax_error() {
    let error = Config::from_toml_str("[stale\nstale_percent = 70\n").unwrap_err();
    assert!(matches!(error, ConfigError::Syntax(_)), "{error:?}");
    assert!(error.to_string().contains("line 1"), "{error}");
}

#[test]
fn validation_failures_list_every_issue() {
    let error = Config::from_toml_str(&fixture("invalid_rules.toml")).unwrap_err();
    let ConfigError::Invalid(issues) = &error else {
        panic!("expected validation failure, got {error:?}");
    };
    let keys: Vec<_> = issues.iter().map(|issue| issue.key.as_str()).collect();
    assert_eq!(
        keys,
        [
            "stale.stale_percent",
            "stale.luma_delta_threshold",
            "prompt.snooze_presets_minutes",
            "action.command"
        ]
    );
    let message = error.to_string();
    assert!(
        message.starts_with("invalid config:\n  stale.stale_percent: "),
        "{message}"
    );
    assert_eq!(message.lines().count(), 5, "{message}");
}

#[test]
fn load_reads_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "[history]\nmax_entries = 50\n").unwrap();
    let outcome = Config::load(&path).unwrap();
    assert_eq!(outcome.config.history.max_entries, 50);
}

#[test]
fn load_reports_missing_file_distinctly() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let error = Config::load(&path).unwrap_err();
    let ConfigError::NotFound { path: missing } = &error else {
        panic!("expected not found, got {error:?}");
    };
    assert_eq!(missing, &path);
    assert!(error.to_string().starts_with("config file not found: "));
}

#[test]
fn load_reports_other_read_failures_as_io() {
    let dir = tempfile::tempdir().unwrap();
    let error = Config::load(dir.path()).unwrap_err();
    assert!(matches!(error, ConfigError::Io { .. }), "{error:?}");
    assert!(error.to_string().starts_with("failed to read "));
}
