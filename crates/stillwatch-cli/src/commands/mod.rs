//! Subcommand handlers and dispatch.

pub mod config;
pub mod daemon;
pub mod diagnostics;

use crate::args::{Command, ConfigCommand};

/// Runs the handler for `command`.
///
/// # Errors
///
/// Returns whatever the handler returns, including "isn't available yet" for
/// commands that aren't implemented.
pub fn dispatch(command: Command) -> anyhow::Result<()> {
    match command {
        Command::Status(args) => daemon::status(&args),
        Command::Snooze(args) => daemon::snooze(&args),
        Command::CancelSnooze => daemon::cancel_snooze(),
        Command::Pause => daemon::pause(),
        Command::Resume => daemon::resume(),
        Command::Reload => daemon::reload(),
        Command::History(args) => daemon::history(&args),
        Command::Probe(args) => diagnostics::probe(&args),
        Command::IdleTest(args) => diagnostics::idle_test(&args),
        Command::Config { command } => match command {
            ConfigCommand::Init(args) => config::init(&args),
            ConfigCommand::Check(args) => config::check(&args),
        },
    }
}

fn unavailable(command: &str) -> anyhow::Error {
    anyhow::anyhow!("`stillwatch {command}` isn't available yet")
}

#[cfg(test)]
mod tests {
    use clap::Parser as _;

    use super::*;
    use crate::args::Cli;

    fn dispatch_args(args: &[&str]) -> anyhow::Result<()> {
        let cli = Cli::try_parse_from(std::iter::once("stillwatch").chain(args.iter().copied()))?;
        dispatch(cli.command)
    }

    #[test]
    fn unimplemented_commands_say_so() {
        let cases: [(&[&str], &str); 11] = [
            (&["status"], "status"),
            (&["snooze", "45m"], "snooze"),
            (&["cancel-snooze"], "cancel-snooze"),
            (&["pause"], "pause"),
            (&["resume"], "resume"),
            (&["reload"], "reload"),
            (&["history", "--since", "2h"], "history"),
            (&["probe"], "probe"),
            (&["idle-test"], "idle-test"),
            (&["config", "init"], "config init"),
            (&["config", "check"], "config check"),
        ];
        for (args, name) in cases {
            let err = dispatch_args(args).unwrap_err();
            assert_eq!(
                err.to_string(),
                format!("`stillwatch {name}` isn't available yet")
            );
        }
    }
}
