use std::path::PathBuf;

use super::*;

fn write_config(contents: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(paths::CONFIG_FILE);
    std::fs::write(&path, contents).unwrap();
    (dir, path)
}

#[test]
fn reads_and_parses_file() {
    let (_dir, path) = write_config("[history]\nmax_entries = 50\n");
    let outcome = load(&path).unwrap();
    assert_eq!(outcome.config.history.max_entries, 50);
    assert_eq!(outcome.migrated_from, None);
}

#[test]
fn written_defaults_load_back() {
    let (_dir, path) = write_config(&Config::default().to_toml_string().unwrap());
    assert_eq!(load(&path).unwrap().config, Config::default());
}

#[test]
fn missing_file_is_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(paths::CONFIG_FILE);
    let error = load(&path).unwrap_err();
    let ConfigError::NotFound { path: missing } = &error else {
        panic!("expected not found, got {error:?}");
    };
    assert_eq!(missing, &path);
    assert!(error.to_string().starts_with("config file not found: "));
}

#[test]
fn other_read_failures_are_io() {
    let dir = tempfile::tempdir().unwrap();
    let error = load(dir.path()).unwrap_err();
    let ConfigError::Io { path, .. } = &error else {
        panic!("expected io error, got {error:?}");
    };
    assert_eq!(path, dir.path());
    assert!(error.to_string().starts_with("failed to read "));
}

#[test]
fn parse_and_validation_errors_pass_through() {
    let (_dir, path) = write_config("[stale]\nstale_percent = 0\n");
    assert!(matches!(load(&path), Err(ConfigError::Invalid(_))));
    let (_dir, path) = write_config("version = 2\n");
    assert!(matches!(
        load(&path),
        Err(ConfigError::VersionTooNew { found: 2, .. })
    ));
}

#[test]
fn load_default_reads_the_standard_path() {
    let expected = paths::config_file().map(|path| format!("{:?}", load(&path)));
    let actual = load_default();
    match (expected, actual) {
        (Ok(expected), Ok(outcome)) => assert_eq!(expected, format!("{:?}", Ok::<_, ()>(outcome))),
        (Ok(expected), Err(ConfigFileError::Config(error))) => {
            assert_eq!(expected, format!("{:?}", Err::<(), _>(error)));
        }
        (Err(expected), Err(ConfigFileError::Paths(error))) => assert_eq!(expected, error),
        (expected, actual) => panic!("{expected:?} vs {actual:?}"),
    }
}

#[test]
fn errors_display_their_source() {
    let paths = ConfigFileError::from(PathsError::NoConfigDir);
    assert_eq!(paths.to_string(), PathsError::NoConfigDir.to_string());
    let config = ConfigFileError::from(ConfigError::NotFound {
        path: PathBuf::from("/nope/config.toml"),
    });
    assert_eq!(
        config.to_string(),
        "config file not found: /nope/config.toml"
    );
}
