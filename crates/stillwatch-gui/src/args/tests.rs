use clap::{CommandFactory, Parser};

use super::{Cli, Command};
use crate::launch::LaunchMode;

fn parse(args: &[&str]) -> Cli {
    Cli::try_parse_from(std::iter::once("stillwatch-gui").chain(args.iter().copied())).unwrap()
}

#[test]
fn command_definition_is_valid() {
    Cli::command().debug_assert();
}

#[test]
fn no_subcommand_is_the_tray() {
    let cli = parse(&[]);
    assert_eq!(cli.command, None);
    assert_eq!(cli.launch_mode(), LaunchMode::Tray);
    assert_eq!(cli.bus_address, None);
    assert_eq!(cli.log_level, None);
}

#[test]
fn settings_and_prompt_open_those_windows() {
    assert_eq!(parse(&["settings"]).command, Some(Command::Settings));
    assert_eq!(parse(&["settings"]).launch_mode(), LaunchMode::Settings);
    assert_eq!(parse(&["prompt"]).launch_mode(), LaunchMode::Prompt);
}

#[test]
fn bus_address_and_log_level_are_global() {
    let cli = parse(&[
        "--bus-address",
        "unix:path=/tmp/stillwatch-bus",
        "--log-level",
        "debug",
        "prompt",
    ]);
    assert_eq!(
        cli.bus_address.as_deref(),
        Some("unix:path=/tmp/stillwatch-bus")
    );
    assert_eq!(cli.log_level.as_deref(), Some("debug"));
    assert_eq!(cli.launch_mode(), LaunchMode::Prompt);
}
