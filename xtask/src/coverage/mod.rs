//! Tests under `cargo llvm-cov nextest`, plus line coverage thresholds.
//!
//! The thresholds and the ignore regex live only here; the CI workflow runs
//! `cargo xtask coverage` rather than repeating them.

mod report;

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use self::report::{CrateDir, Thresholds};
use crate::process::Step;

/// Files left out of coverage: binary `main.rs` wiring and the xtask crate itself.
const IGNORE_FILENAME_REGEX: &str = r"crates/[^/]+/src/main\.rs$|/xtask/";

const THRESHOLDS: Thresholds<'static> = Thresholds {
    workspace: 80,
    crates: &[("stillwatch-core", 90)],
};

/// Where the lcov report is written, relative to the workspace root.
const LCOV_PATH: &str = "target/coverage/lcov.info";

const SUMMARY_PATH: &str = "target/coverage/summary.json";

/// The commands that run the tests and write the lcov and JSON reports.
fn steps() -> [Step; 3] {
    let report = |format: &str, path: &str| {
        Step::new(
            "cargo",
            [
                "llvm-cov",
                "report",
                format,
                "--output-path",
                path,
                "--ignore-filename-regex",
                IGNORE_FILENAME_REGEX,
            ],
        )
    };
    let mut summary = report("--json", SUMMARY_PATH);
    summary.args.push("--summary-only".to_owned());
    [
        Step::new(
            "cargo",
            [
                "llvm-cov",
                "nextest",
                "--workspace",
                "--locked",
                "--no-report",
            ],
        ),
        report("--lcov", LCOV_PATH),
        summary,
    ]
}

/// Runs the workspace tests with coverage and fails if a threshold is missed.
pub fn run(root: &Path) -> Result<()> {
    let out_dir = root.join(LCOV_PATH);
    if let Some(dir) = out_dir.parent() {
        fs::create_dir_all(dir).with_context(|| format!("failed to create {}", dir.display()))?;
    }
    for step in steps() {
        step.run(root)?;
    }
    let summary = root.join(SUMMARY_PATH);
    let json = fs::read_to_string(&summary)
        .with_context(|| format!("failed to read {}", summary.display()))?;
    let metadata = Step::new(
        "cargo",
        ["metadata", "--format-version", "1", "--no-deps", "--locked"],
    )
    .output(root)?;
    let totals = report::aggregate(&json, &crate_dirs(&metadata)?)?;
    eprint!("{}", report::render(&totals));
    eprintln!("lcov report: {LCOV_PATH}");

    let violations = report::evaluate(&totals, THRESHOLDS);
    for violation in &violations {
        eprintln!(
            "{}: {} line coverage ({}/{}), needs {}%",
            violation.scope,
            violation.lines.percent(),
            violation.lines.covered,
            violation.lines.count,
            violation.min
        );
    }
    if !violations.is_empty() {
        bail!(
            "line coverage is below threshold for {} scope(s)",
            violations.len()
        );
    }
    Ok(())
}

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
}

#[derive(Deserialize)]
struct Package {
    name: String,
    manifest_path: PathBuf,
}

/// Workspace packages and their directories, from `cargo metadata --no-deps` output.
fn crate_dirs(metadata: &str) -> Result<Vec<CrateDir>> {
    let metadata: Metadata = serde_json::from_str(metadata).context("invalid cargo metadata")?;
    Ok(metadata
        .packages
        .into_iter()
        .filter_map(|package| {
            let dir = package.manifest_path.parent()?.to_path_buf();
            Some(CrateDir {
                name: package.name,
                dir,
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::{CrateDir, crate_dirs, steps};

    #[test]
    fn steps_share_one_ignore_regex() {
        let [test, lcov, summary] = steps().map(|step| step.to_string());
        assert_eq!(
            test,
            "cargo llvm-cov nextest --workspace --locked --no-report"
        );
        assert_eq!(
            lcov,
            r"cargo llvm-cov report --lcov --output-path target/coverage/lcov.info --ignore-filename-regex 'crates/[^/]+/src/main\.rs$|/xtask/'"
        );
        assert_eq!(
            summary,
            r"cargo llvm-cov report --json --output-path target/coverage/summary.json --ignore-filename-regex 'crates/[^/]+/src/main\.rs$|/xtask/' --summary-only"
        );
    }

    #[test]
    fn parses_crate_dirs_from_metadata() {
        let json = r#"{"packages": [
            {"name": "stillwatch-core", "manifest_path": "/ws/crates/stillwatch-core/Cargo.toml", "version": "0.1.0"},
            {"name": "xtask", "manifest_path": "/ws/xtask/Cargo.toml"}
        ], "workspace_root": "/ws"}"#;
        assert_eq!(
            crate_dirs(json).unwrap(),
            [
                CrateDir {
                    name: "stillwatch-core".into(),
                    dir: "/ws/crates/stillwatch-core".into()
                },
                CrateDir {
                    name: "xtask".into(),
                    dir: "/ws/xtask".into()
                },
            ]
        );
        assert!(crate_dirs("[]").is_err());
    }
}
