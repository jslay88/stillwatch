use std::fs;
use std::path::{Path, PathBuf};

use super::count::ModDecl;
use super::fixtures::{code, inline_tests};
use super::{DEFAULT_MAX_LINES, Offender, analyze_tree, check, offenders, resolve};

fn write(root: &Path, rel: &str, contents: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn offender_names(root: &Path) -> Vec<(String, usize)> {
    let analyses = analyze_tree(root).unwrap();
    offenders(&analyses, DEFAULT_MAX_LINES)
        .into_iter()
        .map(|Offender { path, counted }| {
            (
                path.strip_prefix(root).unwrap().display().to_string(),
                counted,
            )
        })
        .collect()
}

#[test]
fn boundary_at_the_limit() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "src/exact.rs", &code(400));
    write(dir.path(), "src/over.rs", &code(401));
    assert_eq!(
        offender_names(dir.path()),
        [("src/over.rs".to_owned(), 401)]
    );

    let err = check(dir.path(), DEFAULT_MAX_LINES).unwrap_err();
    assert!(
        err.to_string().starts_with("1 file(s) exceed 400 lines"),
        "{err}"
    );
    assert!(check(dir.path(), 401).is_ok());
}

#[test]
fn inline_tests_do_not_count() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "src/lib.rs",
        &format!("{}{}", code(300), inline_tests(500)),
    );
    assert!(check(dir.path(), DEFAULT_MAX_LINES).is_ok());
}

#[test]
fn sibling_test_files_are_not_limited() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "src/lib.rs", "mod big;\n#[cfg(test)]\nmod tests;\n");
    write(
        root,
        "src/tests.rs",
        &format!("mod helpers;\n{}", code(500)),
    );
    write(root, "src/tests/helpers.rs", &code(500));
    write(root, "src/big.rs", "#[cfg(test)]\nmod tests;\n");
    write(root, "src/big/tests.rs", &code(500));
    write(root, "src/stray.rs", &code(401));
    assert_eq!(offender_names(root), [("src/stray.rs".to_owned(), 401)]);
}

#[test]
fn skips_target_and_hidden_dirs() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "target/debug/build/out.rs", &code(500));
    write(dir.path(), ".git/x.rs", &code(500));
    write(dir.path(), "node_modules/x.rs", &code(500));
    write(dir.path(), "src/notes.txt", &code(500));
    assert!(analyze_tree(dir.path()).unwrap().is_empty());
}

#[test]
fn parse_errors_name_the_file() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "src/broken.rs", "fn {");
    let err = check(dir.path(), DEFAULT_MAX_LINES).unwrap_err();
    assert!(err.to_string().contains("broken.rs"), "{err}");
}

fn decl(parents: &[&str], name: &str, path: Option<&str>) -> ModDecl {
    ModDecl {
        parents: parents.iter().map(|&p| p.to_owned()).collect(),
        name: name.to_owned(),
        path: path.map(str::to_owned),
        test_only: true,
    }
}

fn resolved(file: &str, decl: &ModDecl, existing: &[&str]) -> Option<PathBuf> {
    resolve(Path::new(file), decl, |p| {
        existing.iter().any(|e| Path::new(e) == p)
    })
}

#[test]
fn resolves_module_files() {
    let tests = decl(&[], "tests", None);
    let found = |file, existing: &[&str]| resolved(file, &tests, existing);

    assert_eq!(
        found("src/lib.rs", &["src/tests.rs"]),
        Some("src/tests.rs".into())
    );
    assert_eq!(
        found("src/lib.rs", &["src/tests/mod.rs"]),
        Some("src/tests/mod.rs".into())
    );
    assert_eq!(
        found("src/a.rs", &["src/a/tests.rs", "src/tests.rs"]),
        Some("src/a/tests.rs".into())
    );
    assert_eq!(
        found("src/bin/x.rs", &["src/bin/tests.rs"]),
        Some("src/bin/tests.rs".into())
    );
    assert_eq!(found("src/lib.rs", &[]), None);

    let nested = decl(&["a", "b"], "c", None);
    assert_eq!(
        resolved("src/m/mod.rs", &nested, &["src/m/a/b/c.rs"]),
        Some("src/m/a/b/c.rs".into())
    );

    let pathed = decl(&[], "t", Some("t_impl.rs"));
    assert_eq!(
        resolved("src/a.rs", &pathed, &["src/t_impl.rs"]),
        Some("src/t_impl.rs".into())
    );
    let nested_pathed = decl(&["inner"], "t", Some("t.rs"));
    assert_eq!(
        resolved("src/lib.rs", &nested_pathed, &["src/inner/t.rs"]),
        Some("src/inner/t.rs".into())
    );
}
