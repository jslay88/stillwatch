//! Aggregating an llvm-cov JSON summary per crate and checking thresholds.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::Deserialize;

/// A workspace crate and the directory holding its `Cargo.toml`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CrateDir {
    /// Package name.
    pub name: String,
    /// Absolute directory of the package.
    pub dir: PathBuf,
}

/// Line counts from llvm-cov.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct Lines {
    /// Instrumented lines.
    pub count: u64,
    /// Instrumented lines that ran at least once.
    pub covered: u64,
}

impl Lines {
    fn add(&mut self, other: Self) {
        self.count += other.count;
        self.covered += other.covered;
    }

    /// Whether coverage is at least `min_percent`. Nothing instrumented passes.
    pub fn meets(self, min_percent: u64) -> bool {
        self.covered * 100 >= min_percent * self.count
    }

    /// Coverage as a percentage with one decimal, or `-` when nothing is instrumented.
    pub fn percent(self) -> String {
        if self.count == 0 {
            return "-".to_owned();
        }
        let tenths = self.covered * 1000 / self.count;
        format!("{}.{}%", tenths / 10, tenths % 10)
    }
}

/// Minimum line coverage, in whole percent.
#[derive(Clone, Copy, Debug)]
pub struct Thresholds<'a> {
    /// Applies to the workspace as a whole.
    pub workspace: u64,
    /// Applies to individual crates, by package name.
    pub crates: &'a [(&'a str, u64)],
}

/// Line coverage for the workspace and each crate in it.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Totals {
    /// Every reported file.
    pub workspace: Lines,
    /// Files grouped by the crate that owns them.
    pub crates: BTreeMap<String, Lines>,
}

/// A scope whose coverage is below its threshold.
#[derive(Debug, PartialEq, Eq)]
pub struct Violation {
    /// `workspace` or a crate name.
    pub scope: String,
    /// The measured lines.
    pub lines: Lines,
    /// The threshold that was missed.
    pub min: u64,
}

#[derive(Deserialize)]
struct Export {
    data: Vec<ExportData>,
}

#[derive(Deserialize)]
struct ExportData {
    files: Vec<FileSummary>,
}

#[derive(Deserialize)]
struct FileSummary {
    filename: PathBuf,
    summary: Summary,
}

#[derive(Deserialize)]
struct Summary {
    lines: Lines,
}

/// Sums the per-file line counts of `cargo llvm-cov report --json --summary-only`
/// into workspace and per-crate totals. Every crate gets an entry, even with no lines.
pub fn aggregate(json: &str, crates: &[CrateDir]) -> Result<Totals> {
    let export: Export = serde_json::from_str(json).context("invalid llvm-cov JSON summary")?;
    let mut totals = Totals {
        workspace: Lines::default(),
        crates: crates
            .iter()
            .map(|c| (c.name.clone(), Lines::default()))
            .collect(),
    };
    for file in export.data.iter().flat_map(|data| &data.files) {
        totals.workspace.add(file.summary.lines);
        let owner = crates
            .iter()
            .filter(|c| file.filename.starts_with(&c.dir))
            .max_by_key(|c| c.dir.components().count());
        if let Some(owner) = owner
            && let Some(lines) = totals.crates.get_mut(&owner.name)
        {
            lines.add(file.summary.lines);
        }
    }
    Ok(totals)
}

/// Every scope below its threshold. Crates without a threshold are not checked.
pub fn evaluate(totals: &Totals, thresholds: Thresholds<'_>) -> Vec<Violation> {
    let workspace = ("workspace", Some(totals.workspace), thresholds.workspace);
    let crates = thresholds
        .crates
        .iter()
        .map(|&(name, min)| (name, totals.crates.get(name).copied(), min));
    std::iter::once(workspace)
        .chain(crates)
        .filter_map(|(scope, lines, min)| {
            let lines = lines.unwrap_or_default();
            (!lines.meets(min)).then(|| Violation {
                scope: scope.to_owned(),
                lines,
                min,
            })
        })
        .collect()
}

/// A table of covered/instrumented lines per crate and for the workspace.
pub fn render(totals: &Totals) -> String {
    let width = totals
        .crates
        .keys()
        .map(String::len)
        .max()
        .unwrap_or(0)
        .max("workspace".len());
    let mut out = String::new();
    let rows = totals
        .crates
        .iter()
        .map(|(name, lines)| (name.as_str(), *lines));
    for (name, lines) in rows.chain(std::iter::once(("workspace", totals.workspace))) {
        let counts = format!("{}/{}", lines.covered, lines.count);
        let _ = writeln!(out, "{name:<width$}  {counts:>11}  {:>6}", lines.percent());
    }
    out
}

#[cfg(test)]
mod tests;
