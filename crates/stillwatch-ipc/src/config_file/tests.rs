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

fn entries(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect()
}

#[test]
fn write_creates_parent_directories() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a/b").join(paths::CONFIG_FILE);
    write(&path, "version = 1\n", false).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "version = 1\n");
    assert_eq!(entries(path.parent().unwrap()), [paths::CONFIG_FILE]);
}

#[test]
fn write_refuses_to_overwrite_without_permission() {
    let (dir, path) = write_config("# mine\n");
    let error = write(&path, "version = 1\n", false).unwrap_err();
    assert!(matches!(&error, WriteError::Exists { path: existing } if existing == &path));
    assert_eq!(
        error.to_string(),
        format!("{} already exists", path.display())
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "# mine\n");
    assert_eq!(entries(dir.path()), [paths::CONFIG_FILE]);
}

#[test]
fn write_overwrites_when_allowed_and_keeps_permissions() {
    use std::os::unix::fs::PermissionsExt as _;

    let (_dir, path) = write_config("# mine\n");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
    write(&path, "version = 1\n", true).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "version = 1\n");
    let mode = std::fs::metadata(&path).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o640);
}

#[test]
fn write_follows_a_symlinked_config() {
    let (dir, target) = write_config("# dotfiles\n");
    let link = dir.path().join("link.toml");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    write(&link, "version = 1\n", true).unwrap();
    assert!(
        std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "version = 1\n");
}

#[test]
fn write_to_an_empty_path_fails() {
    let error = write(Path::new(""), "", false).unwrap_err();
    assert!(matches!(error, WriteError::Io { .. }), "{error:?}");
}

#[test]
fn write_reports_io_failures() {
    let (_dir, file) = write_config("");
    let error = write(&file.join(paths::CONFIG_FILE), "", false).unwrap_err();
    let WriteError::Io { path, .. } = &error else {
        panic!("expected an I/O error, got {error:?}");
    };
    assert_eq!(path, &file.join(paths::CONFIG_FILE));
    assert!(error.to_string().starts_with("failed to write "));
}
