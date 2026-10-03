//! The quality gates and the commands behind them.

use std::path::Path;

use anyhow::Result;
use clap::ValueEnum;

use crate::process::Step;
use crate::{coverage, packaging, size};

/// One quality gate, as run by `cargo xtask ci` and the CI workflow.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Gate {
    /// `cargo fmt --check` over the whole workspace.
    Fmt,
    /// `cargo clippy` on all targets with warnings denied.
    Clippy,
    /// The per-file line limit, see [`size`].
    Size,
    /// Copy/paste detection with `jscpd`, configured by `.jscpd.json`.
    Jscpd,
    /// Licenses, advisories, bans, and sources via `cargo deny`, configured by `deny.toml`.
    Deny,
    /// Unused dependencies via `cargo machete`.
    Machete,
    /// Tests under `cargo llvm-cov nextest` plus coverage thresholds, see [`coverage`].
    Coverage,
    /// The systemd unit, desktop files, example hooks, and PKGBUILD.
    Packaging,
    /// Builds the benchmarks without running them.
    Bench,
}

impl Gate {
    /// Every gate, in the order `cargo xtask ci` runs them.
    pub const ALL: [Self; 9] = [
        Self::Fmt,
        Self::Clippy,
        Self::Size,
        Self::Jscpd,
        Self::Deny,
        Self::Machete,
        Self::Packaging,
        Self::Coverage,
        Self::Bench,
    ];

    /// The gates cheap enough for the pre-commit hook.
    pub const FAST: [Self; 3] = [Self::Fmt, Self::Clippy, Self::Size];

    /// Name used on the command line and in progress output.
    pub fn name(self) -> &'static str {
        match self {
            Self::Fmt => "fmt",
            Self::Clippy => "clippy",
            Self::Size => "size",
            Self::Jscpd => "jscpd",
            Self::Deny => "deny",
            Self::Machete => "machete",
            Self::Coverage => "coverage",
            Self::Packaging => "packaging",
            Self::Bench => "bench",
        }
    }

    /// Executables that must be on `PATH` for the gate to run.
    pub fn tools(self) -> &'static [&'static str] {
        match self {
            Self::Fmt => &["cargo-fmt"],
            Self::Clippy => &["cargo-clippy"],
            Self::Size | Self::Bench => &["cargo"],
            Self::Jscpd => &["jscpd"],
            Self::Deny => &["cargo-deny"],
            Self::Machete => &["cargo-machete"],
            Self::Coverage => &["cargo-llvm-cov", "cargo-nextest"],
            Self::Packaging => &["systemd-analyze", "desktop-file-validate"],
        }
    }

    /// The external command for gates that are a single command, `None` for the rest.
    pub fn command(self) -> Option<Step> {
        let step = match self {
            Self::Fmt => Step::new("cargo", ["fmt", "--all", "--", "--check"]),
            Self::Clippy => Step::new(
                "cargo",
                [
                    "clippy",
                    "--workspace",
                    "--all-targets",
                    "--locked",
                    "--",
                    "-D",
                    "warnings",
                ],
            ),
            Self::Jscpd => Step::new("jscpd", ["--config", ".jscpd.json", "."]),
            Self::Deny => Step::new("cargo", ["deny", "--locked", "check"]),
            Self::Machete => Step::new("cargo", ["machete"]),
            Self::Bench => Step::new("cargo", ["bench", "--workspace", "--locked", "--no-run"]),
            Self::Size | Self::Coverage | Self::Packaging => return None,
        };
        Some(step)
    }

    /// Runs the gate from the workspace root.
    pub fn run(self, root: &Path) -> Result<()> {
        match self {
            Self::Size => size::check(root, size::DEFAULT_MAX_LINES),
            Self::Coverage => coverage::run(root),
            Self::Packaging => packaging::check(root),
            _ => self.command().map_or(Ok(()), |step| step.run(root)),
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::ValueEnum;

    use super::Gate;

    fn command_line(gate: Gate) -> Option<String> {
        gate.command().map(|step| step.to_string())
    }

    #[test]
    fn single_command_gates() {
        let expected = [
            (Gate::Fmt, "cargo fmt --all -- --check"),
            (
                Gate::Clippy,
                "cargo clippy --workspace --all-targets --locked -- -D warnings",
            ),
            (Gate::Jscpd, "jscpd --config .jscpd.json ."),
            (Gate::Deny, "cargo deny --locked check"),
            (Gate::Machete, "cargo machete"),
            (Gate::Bench, "cargo bench --workspace --locked --no-run"),
        ];
        for (gate, line) in expected {
            assert_eq!(command_line(gate).as_deref(), Some(line), "{gate:?}");
        }
    }

    #[test]
    fn in_process_gates_have_no_single_command() {
        assert_eq!(command_line(Gate::Size), None);
        assert_eq!(command_line(Gate::Coverage), None);
        assert_eq!(command_line(Gate::Packaging), None);
    }

    #[test]
    fn ci_order_matches_ci_workflow() {
        let names: Vec<_> = Gate::ALL.iter().map(|gate| gate.name()).collect();
        assert_eq!(
            names,
            [
                "fmt",
                "clippy",
                "size",
                "jscpd",
                "deny",
                "machete",
                "packaging",
                "coverage",
                "bench",
            ]
        );
        assert_eq!(Gate::FAST, Gate::ALL[..3]);
    }

    #[test]
    fn names_match_cli_values() {
        for gate in Gate::ALL {
            assert_eq!(Gate::from_str(gate.name(), false), Ok(gate));
            assert_ne!(gate.tools(), [""; 0]);
        }
    }
}
