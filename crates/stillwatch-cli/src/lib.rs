//! The `stillwatch` command-line tool.

pub mod args;
pub mod commands;
pub mod duration;

use stillwatch_ipc::logging::{self, LogTarget};

pub use args::Cli;

/// The CLI only reports problems unless asked for more.
pub const DEFAULT_LOG_LEVEL: &str = "warn";

/// Sets up stderr logging and runs the requested command.
///
/// # Errors
///
/// Fails if logging was already initialized, or if the command fails.
pub fn run(cli: Cli) -> anyhow::Result<()> {
    logging::init_with_override(
        cli.log_level.as_deref(),
        DEFAULT_LOG_LEVEL,
        LogTarget::Stderr,
    )?;
    commands::dispatch(cli.command)
}

#[cfg(test)]
mod tests {
    use clap::Parser as _;

    use super::*;

    #[test]
    fn run_initializes_logging_then_dispatches() {
        let cli = Cli::try_parse_from(["stillwatch", "pause"]).unwrap();
        let err = run(cli.clone()).unwrap_err();
        assert_eq!(err.to_string(), "`stillwatch pause` isn't available yet");

        let err = run(cli).unwrap_err();
        assert_eq!(err.to_string(), "logging is already initialized");
    }
}
