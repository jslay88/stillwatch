//! `Config::load` against the fixture files, through the public API only.

use std::path::PathBuf;

use stillwatch_core::config::{CURRENT_VERSION, Config, ConfigError};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/config")
        .join(name)
}

#[test]
fn documented_default_config_loads_as_default() {
    let outcome = Config::load(&fixture("default.toml")).unwrap();
    assert_eq!(outcome.config, Config::default());
    assert_eq!(outcome.migrated_from, None);
}

#[test]
fn written_defaults_load_back() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, Config::default().to_toml_string().unwrap()).unwrap();
    assert_eq!(Config::load(&path).unwrap().config, Config::default());
}

#[test]
fn partial_config_fills_in_defaults() {
    let config = Config::load(&fixture("partial_stale.toml")).unwrap().config;
    assert_eq!(config.version, CURRENT_VERSION);
    assert_eq!(config.stale.stale_percent, 80);
    assert_eq!(config.stale.ignore_regions.len(), 1);
    assert_eq!(config.prompt, Config::default().prompt);
}

#[test]
fn newer_version_is_refused() {
    let error = Config::load(&fixture("version_too_new.toml")).unwrap_err();
    assert!(
        matches!(error, ConfigError::VersionTooNew { found: 2, .. }),
        "{error:?}"
    );
}

#[test]
fn unsupported_old_version_is_refused() {
    let error = Config::load(&fixture("v0_threshold_rename.toml")).unwrap_err();
    assert!(
        matches!(error, ConfigError::VersionTooOld { found: 0, .. }),
        "{error:?}"
    );
}

#[test]
fn broken_rules_are_all_reported() {
    let error = Config::load(&fixture("invalid_rules.toml")).unwrap_err();
    let ConfigError::Invalid(issues) = error else {
        panic!("expected validation failure, got {error:?}");
    };
    assert_eq!(issues.len(), 4);
}

#[test]
fn missing_file_is_not_found() {
    let error = Config::load(&fixture("does_not_exist.toml")).unwrap_err();
    assert!(matches!(error, ConfigError::NotFound { .. }), "{error:?}");
}
