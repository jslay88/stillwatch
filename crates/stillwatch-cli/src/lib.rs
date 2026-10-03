//! The `stillwatch` command-line tool.
//!
//! Daemon commands go through [`commands::daemon::execute`], which takes the
//! bus address explicitly so tests can point it at a private bus. Output
//! formatting lives in [`render`], and exit codes in [`exit`].

pub mod args;
pub mod commands;
pub mod connect;
pub mod duration;
pub mod exit;
pub mod render;

use std::io;

use stillwatch_ipc::logging::{self, LogTarget};

pub use args::Cli;
use render::Style;

/// The CLI only reports problems unless asked for more.
pub const DEFAULT_LOG_LEVEL: &str = "warn";

/// Runs `cli`, prints any failure to stderr, and returns the exit code (see
/// [`exit`]).
#[must_use]
pub fn run(cli: &Cli) -> u8 {
    let result = try_run(cli);
    exit::report(&result, &mut io::stderr().lock())
}

/// Sets up stderr logging and runs the requested command against stdout.
///
/// # Errors
///
/// Fails if logging was already initialized, or if the command fails.
pub fn try_run(cli: &Cli) -> anyhow::Result<()> {
    logging::init_with_override(
        cli.log_level.as_deref(),
        DEFAULT_LOG_LEVEL,
        LogTarget::Stderr,
    )?;
    commands::dispatch(cli, &Style::detect(), &mut io::stdout().lock())
}

#[cfg(test)]
mod tests {
    use clap::Parser as _;

    use super::*;

    #[test]
    fn run_initializes_logging_then_dispatches() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing.toml");
        let missing = missing.to_str().unwrap();
        let cli = Cli::try_parse_from(["stillwatch", "config", "check", missing]).unwrap();
        let err = try_run(&cli).unwrap_err();
        assert!(err.to_string().contains("doesn't exist"), "{err}");

        assert_eq!(run(&cli), exit::FAILED);
        let err = try_run(&cli).unwrap_err();
        assert_eq!(err.to_string(), "logging is already initialized");
    }
}
