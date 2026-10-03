//! The `stillwatch` command-line tool.

use std::process::ExitCode;

use clap::Parser as _;

fn main() -> ExitCode {
    ExitCode::from(stillwatch_cli::run(&stillwatch_cli::Cli::parse()))
}
