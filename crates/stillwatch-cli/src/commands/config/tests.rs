use std::fs;

use stillwatch_core::config::{Config, MigrationNote};

use super::*;

fn run(f: impl FnOnce(&mut dyn Write) -> anyhow::Result<()>) -> (anyhow::Result<()>, String) {
    let mut out = Vec::new();
    let result = f(&mut out);
    (result, String::from_utf8(out).unwrap())
}

#[test]
fn init_writes_commented_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested/config.toml");
    let (result, out) = run(|out| init_at(&path, false, out));
    result.unwrap();
    assert_eq!(out, format!("wrote {}\n", path.display()));
    let written = fs::read_to_string(&path).unwrap();
    assert_eq!(written, schema::commented_toml().unwrap());
}

#[test]
fn init_refuses_to_overwrite_without_force() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "# mine\n").unwrap();
    let (result, out) = run(|out| init_at(&path, false, out));
    let err = result.unwrap_err().to_string();
    assert_eq!(
        err,
        format!(
            "{} already exists; pass --force to overwrite it",
            path.display()
        )
    );
    assert_eq!(out, "");
    assert_eq!(fs::read_to_string(&path).unwrap(), "# mine\n");

    let (result, _) = run(|out| init_at(&path, true, out));
    result.unwrap();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        schema::commented_toml().unwrap()
    );
}

#[test]
fn init_reports_write_failures() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("file");
    fs::write(&file, "").unwrap();
    let (result, _) = run(|out| init_at(&file.join("config.toml"), false, out));
    assert!(
        result
            .unwrap_err()
            .to_string()
            .starts_with("failed to write ")
    );
}

#[test]
fn check_accepts_a_valid_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "[stale]\nstale_percent = 80\n").unwrap();
    let (result, out) = run(|out| check_at(&path, out));
    result.unwrap();
    assert_eq!(out, format!("{}: ok\n", path.display()));
}

#[test]
fn check_lists_every_issue_by_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(
        &path,
        "[stale]\nstale_percent = 0\n[history]\nmax_entries = 0\n",
    )
    .unwrap();
    let (result, out) = run(|out| check_at(&path, out));
    assert_eq!(
        result.unwrap_err().to_string(),
        format!("{} is invalid: 2 problems", path.display())
    );
    assert_eq!(
        out,
        "stale.stale_percent: must be between 1 and 100, got 0\n\
         history.max_entries: must be at least 1, got 0\n"
    );
}

#[test]
fn check_reports_a_parse_error_by_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "[stale]\nrequire = \"most\"\n").unwrap();
    let (result, out) = run(|out| check_at(&path, out));
    assert_eq!(
        result.unwrap_err().to_string(),
        format!("{} is invalid: 1 problem", path.display())
    );
    assert!(
        out.starts_with("stale.require: unknown variant `most`"),
        "{out}"
    );
}

#[test]
fn check_reports_whole_file_errors_with_their_cause() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "version = 99\n").unwrap();
    let (result, out) = run(|out| check_at(&path, out));
    let err = result.unwrap_err();
    assert_eq!(err.to_string(), format!("{} is invalid", path.display()));
    assert!(format!("{err:#}").contains("newer than this build supports"));
    assert_eq!(out, "");
}

#[test]
fn check_points_at_init_for_a_missing_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let (result, _) = run(|out| check_at(&path, out));
    assert_eq!(
        result.unwrap_err().to_string(),
        format!(
            "{} doesn't exist; `stillwatch config init` creates it",
            path.display()
        )
    );
}

#[test]
fn valid_report_mentions_migration_notes() {
    let outcome = LoadOutcome {
        config: Config::default(),
        migrated_from: Some(0),
        notes: vec![MigrationNote {
            from: 0,
            to: 1,
            message: "renamed `a` to `b`".to_owned(),
        }],
    };
    let (result, out) = run(|out| report_valid(Path::new("c.toml"), &outcome, out));
    result.unwrap();
    assert_eq!(
        out,
        "migrated from version 0 to 1 in memory; the file is unchanged\n\
         \x20 v0 -> v1: renamed `a` to `b`\n\
         c.toml: ok\n"
    );
}

#[test]
fn default_path_is_the_standard_config_file() {
    assert_eq!(path_or_default(None).ok(), paths::config_file().ok(),);
    assert_eq!(
        path_or_default(Some(Path::new("x.toml"))).unwrap(),
        Path::new("x.toml")
    );
}

#[test]
fn public_handlers_use_the_given_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    init(&ConfigInitArgs {
        force: false,
        path: Some(path.clone()),
    })
    .unwrap();
    check(&ConfigCheckArgs { path: Some(path) }).unwrap();
}
