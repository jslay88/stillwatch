//! The `stillwatch` command-line tool.

use clap::Parser as _;

fn main() -> anyhow::Result<()> {
    stillwatch_cli::run(stillwatch_cli::Cli::parse())
}
