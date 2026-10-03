//! Command-line arguments for `stillwatch`.

use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::time::Duration;

use clap::builder::PossibleValuesParser;
use clap::{Args, Parser, Subcommand};
use stillwatch_ipc::logging::LEVELS;

use crate::duration::{parse_minutes_or_duration, parse_positive};

/// Control and inspect the Stillwatch daemon.
#[derive(Debug, Clone, PartialEq, Eq, Parser)]
#[command(name = "stillwatch", version, propagate_version = true)]
pub struct Cli {
    /// What to do.
    #[command(subcommand)]
    pub command: Command,

    /// Log level for this command's own diagnostics (beats `RUST_LOG`)
    #[arg(long, global = true, value_name = "LEVEL", value_parser = PossibleValuesParser::new(LEVELS))]
    pub log_level: Option<String>,

    /// D-Bus address to find the daemon on instead of the session bus
    #[arg(long, global = true, value_name = "ADDRESS", hide = true)]
    pub bus_address: Option<String>,
}

/// Top-level subcommands.
#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum Command {
    /// Show the daemon's state, idle time, and last stale check
    Status(StatusArgs),
    /// Hold off prompting and blanking for a while
    Snooze(SnoozeArgs),
    /// End the current snooze early
    CancelSnooze,
    /// Stop monitoring until resumed
    Pause,
    /// Resume monitoring after a pause
    Resume,
    /// Make the daemon reload its config file
    Reload,
    /// Show recent decisions (prompts, blanks, snoozes, reloads)
    History(HistoryArgs),
    /// Watch live per-block stale detection to calibrate thresholds
    Probe(ProbeArgs),
    /// Report keyboard, mouse, and gamepad idle transitions as they happen
    IdleTest(IdleTestArgs),
    /// Create or validate the config file
    Config {
        /// Config action.
        #[command(subcommand)]
        command: ConfigCommand,
    },
}

/// Arguments for `stillwatch status`.
#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct StatusArgs {
    /// Print machine-readable JSON
    #[arg(long)]
    pub json: bool,
}

/// Arguments for `stillwatch snooze`.
#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct SnoozeArgs {
    /// How long to snooze, e.g. 45m, 2h, or 1h30m
    #[arg(value_name = "DURATION", value_parser = parse_positive)]
    pub duration: Duration,
}

/// Arguments for `stillwatch history`.
#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct HistoryArgs {
    /// Only show entries newer than this, e.g. 2h
    #[arg(long, value_name = "DURATION", value_parser = parse_positive)]
    pub since: Option<Duration>,

    /// Print entries as JSON lines
    #[arg(long)]
    pub json: bool,
}

/// Arguments for `stillwatch probe`.
#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct ProbeArgs {
    /// Time between captures [default: `stale.check_interval_seconds`]
    #[arg(long, value_name = "DURATION", value_parser = parse_positive)]
    pub interval: Option<Duration>,

    /// Stop after this many samples [default: run until Ctrl-C]
    #[arg(long, value_name = "N")]
    pub count: Option<NonZeroUsize>,

    /// Print each sample as a JSON line
    #[arg(long)]
    pub json: bool,
}

/// Arguments for `stillwatch idle-test`.
#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct IdleTestArgs {
    /// Idle timeout to test with, e.g. 30s or 2m; a bare number is minutes [default: `idle.input_idle_minutes`]
    #[arg(long, visible_alias = "minutes", value_name = "DURATION", value_parser = parse_minutes_or_duration)]
    pub timeout: Option<Duration>,
}

/// `stillwatch config` subcommands.
#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum ConfigCommand {
    /// Write a fully commented default config
    Init(ConfigInitArgs),
    /// Validate a config file without the daemon
    Check(ConfigCheckArgs),
}

/// Arguments for `stillwatch config init`.
#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct ConfigInitArgs {
    /// Overwrite an existing file
    #[arg(long)]
    pub force: bool,

    /// Where to write [default: `$XDG_CONFIG_HOME/stillwatch/config.toml`]
    #[arg(value_name = "PATH")]
    pub path: Option<PathBuf>,
}

/// Arguments for `stillwatch config check`.
#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct ConfigCheckArgs {
    /// File to check [default: `$XDG_CONFIG_HOME/stillwatch/config.toml`]
    #[arg(value_name = "PATH")]
    pub path: Option<PathBuf>,
}

#[cfg(test)]
mod tests;
