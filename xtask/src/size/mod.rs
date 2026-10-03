//! The per-file line limit.
//!
//! Every `.rs` file in the workspace may have at most [`DEFAULT_MAX_LINES`]
//! lines, not counting `#[cfg(test)]` items. Files that are only compiled for
//! tests (a sibling `tests.rs` pulled in with `#[cfg(test)] mod tests;`, and
//! anything below it) are not limited at all.

mod count;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use walkdir::{DirEntry, WalkDir};

use self::count::{Analysis, ModDecl, analyze};

/// The line limit CI enforces.
pub const DEFAULT_MAX_LINES: usize = 400;

/// Directory names never searched for sources.
const SKIPPED_DIRS: [&str; 2] = ["target", "node_modules"];

/// A file over the limit.
#[derive(Debug, PartialEq, Eq)]
struct Offender {
    path: PathBuf,
    counted: usize,
}

/// Fails if any `.rs` file under `root` has more than `max` counted lines.
pub fn check(root: &Path, max: usize) -> Result<()> {
    let analyses = analyze_tree(root)?;
    let offenders = offenders(&analyses, max);
    if offenders.is_empty() {
        eprintln!("size: {} files within {max} lines", analyses.len());
        return Ok(());
    }
    for Offender { path, counted } in &offenders {
        let shown = path.strip_prefix(root).unwrap_or(path);
        eprintln!("{}: {counted} lines (limit {max})", shown.display());
    }
    bail!(
        "{} file(s) exceed {max} lines outside #[cfg(test)] items; move tests to a sibling \
         tests.rs or split the module",
        offenders.len()
    )
}

fn analyze_tree(root: &Path) -> Result<BTreeMap<PathBuf, Analysis>> {
    let mut analyses = BTreeMap::new();
    let walker = WalkDir::new(root)
        .into_iter()
        .filter_entry(|entry| !is_skipped_dir(entry));
    for entry in walker {
        let entry = entry.context("failed to walk the workspace")?;
        let path = entry.path();
        if !entry.file_type().is_file() || path.extension().is_none_or(|ext| ext != "rs") {
            continue;
        }
        let source = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let parsed =
            analyze(&source).with_context(|| format!("failed to parse {}", path.display()))?;
        analyses.insert(entry.into_path(), parsed);
    }
    Ok(analyses)
}

fn is_skipped_dir(entry: &DirEntry) -> bool {
    entry.depth() > 0
        && entry.file_type().is_dir()
        && entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with('.') || SKIPPED_DIRS.contains(&name))
}

fn offenders(analyses: &BTreeMap<PathBuf, Analysis>, max: usize) -> Vec<Offender> {
    let test_files = test_only_files(analyses);
    analyses
        .iter()
        .filter(|(path, analysis)| analysis.counted > max && !test_files.contains(*path))
        .map(|(path, analysis)| Offender {
            path: path.clone(),
            counted: analysis.counted,
        })
        .collect()
}

/// Files reachable only through `#[cfg(test)]` module declarations.
fn test_only_files(analyses: &BTreeMap<PathBuf, Analysis>) -> BTreeSet<PathBuf> {
    let children = |path: &Path, only_test: bool| -> Vec<PathBuf> {
        analyses.get(path).map_or_else(Vec::new, |analysis| {
            analysis
                .modules
                .iter()
                .filter(|decl| decl.test_only || !only_test)
                .filter_map(|decl| resolve(path, decl, |p| analyses.contains_key(p)))
                .collect()
        })
    };
    let mut pending: Vec<PathBuf> = analyses
        .keys()
        .flat_map(|path| children(path, true))
        .collect();
    let mut found = BTreeSet::new();
    while let Some(path) = pending.pop() {
        if found.insert(path.clone()) {
            pending.extend(children(&path, false));
        }
    }
    found
}

/// The file a module declaration in `file` refers to, among the ones `exists` accepts.
///
/// Crate roots outside the usual names (`src/bin/x.rs`, `tests/x.rs`) are
/// handled by also trying the other directory convention as a fallback.
fn resolve(file: &Path, decl: &ModDecl, exists: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let dir = file.parent().unwrap_or_else(|| Path::new(""));
    let own_dir = file
        .file_stem()
        .map_or_else(|| dir.to_path_buf(), |stem| dir.join(stem));
    let bases = if owns_directory(file) {
        [dir.to_path_buf(), own_dir]
    } else {
        [own_dir, dir.to_path_buf()]
    };
    let nested = |base: PathBuf| {
        decl.parents
            .iter()
            .fold(base, |base, parent| base.join(parent))
    };
    let candidates: Vec<PathBuf> = match &decl.path {
        Some(path) if decl.parents.is_empty() => vec![dir.join(path)],
        Some(path) => bases
            .into_iter()
            .map(|base| nested(base).join(path))
            .collect(),
        None => bases
            .into_iter()
            .flat_map(|base| {
                let base = nested(base);
                [
                    base.join(format!("{}.rs", decl.name)),
                    base.join(&decl.name).join("mod.rs"),
                ]
            })
            .collect(),
    };
    candidates.into_iter().find(|candidate| exists(candidate))
}

/// Whether `mod x;` in this file resolves next to it rather than in a directory named after it.
fn owns_directory(file: &Path) -> bool {
    file.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| matches!(name, "mod.rs" | "lib.rs" | "main.rs" | "build.rs"))
}

#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod tests;
