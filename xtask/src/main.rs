//! Repository automation for Stillwatch.
//!
//! Every quality gate CI enforces lives here, so `cargo xtask ci` locally and
//! the GitHub Actions jobs run the same code.

mod ci;
mod coverage;
mod gate;
mod hooks;
mod process;
mod size;
mod workspace;

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::gate::Gate;

#[derive(Debug, Parser)]
#[command(
    name = "cargo xtask",
    about = "Stillwatch quality gates and repository tasks"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run every quality gate in CI order, stopping at the first failure.
    Ci {
        /// Run only the fast gates (fmt, clippy, size), as the pre-commit hook does.
        #[arg(long)]
        fast: bool,
        /// Fail instead of skipping a gate whose tool is not installed.
        #[arg(long)]
        strict: bool,
    },
    /// Run the named gates in the order given.
    Gate {
        /// Gates to run.
        #[arg(required = true, value_enum)]
        gates: Vec<Gate>,
        /// Fail instead of skipping a gate whose tool is not installed.
        #[arg(long)]
        strict: bool,
    },
    /// Fail if any `.rs` file has more than `--max` lines outside `#[cfg(test)]` items.
    CheckSize {
        /// Maximum number of counted lines per file.
        #[arg(long, default_value_t = size::DEFAULT_MAX_LINES)]
        max: usize,
    },
    /// Run the test suite under `cargo llvm-cov nextest` and enforce coverage thresholds.
    Coverage,
    /// Point `core.hooksPath` at `.githooks` so the pre-commit hook runs.
    InstallHooks,
}

fn main() -> Result<()> {
    let root = workspace::root()?;
    match Cli::parse().command {
        Command::Ci { fast, strict } => {
            let gates: &[Gate] = if fast { &Gate::FAST } else { &Gate::ALL };
            ci::run(&root, gates, strict)
        }
        Command::Gate { gates, strict } => ci::run(&root, &gates, strict),
        Command::CheckSize { max } => size::check(&root, max),
        Command::Coverage => coverage::run(&root),
        Command::InstallHooks => hooks::install(&root),
    }
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::Cli;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }
}
