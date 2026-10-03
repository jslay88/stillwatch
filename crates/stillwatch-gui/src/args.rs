//! Command-line arguments for `stillwatch-gui`.

use clap::builder::PossibleValuesParser;
use clap::{Parser, Subcommand};
use stillwatch_ipc::logging::LEVELS;

use crate::launch::LaunchMode;

/// Tray icon and settings window for Stillwatch.
#[derive(Debug, Clone, PartialEq, Eq, Parser)]
#[command(name = "stillwatch-gui", version, propagate_version = true)]
pub struct Cli {
    /// What to open. With no subcommand, only the tray is shown until asked.
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Log level for this process (beats `RUST_LOG`).
    #[arg(long, global = true, value_name = "LEVEL", value_parser = PossibleValuesParser::new(LEVELS))]
    pub log_level: Option<String>,

    /// D-Bus address to use instead of the session bus.
    #[arg(long, global = true, value_name = "ADDRESS", hide = true)]
    pub bus_address: Option<String>,
}

/// How to start the GUI.
#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum Command {
    /// Open the settings window.
    Settings,
    /// Open the prompt dialog. It answers over D-Bus and exits.
    Prompt {
        /// Seconds left on the countdown. Without this, the dialog uses the
        /// daemon status and `prompt.countdown_seconds`.
        #[arg(long, value_name = "SECS")]
        remaining: Option<u64>,
        /// Open on the custom duration field.
        #[arg(long)]
        custom: bool,
    },
}

impl Cli {
    /// The launch mode implied by the parsed arguments.
    #[must_use]
    pub const fn launch_mode(&self) -> LaunchMode {
        match &self.command {
            None => LaunchMode::Tray,
            Some(Command::Settings) => LaunchMode::Settings,
            Some(Command::Prompt { .. }) => LaunchMode::Prompt,
        }
    }
}

#[cfg(test)]
mod tests;
