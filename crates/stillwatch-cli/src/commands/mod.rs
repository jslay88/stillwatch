//! Subcommand handlers and dispatch.

pub mod config;
pub mod control;
pub mod daemon;
pub mod diagnostics;
pub mod history;
pub mod idle_test;
pub mod probe;
pub mod reload;
pub mod status;

use std::io::Write;

use crate::args::{Cli, Command, ConfigCommand, IdleTestArgs};
use crate::render::Style;
use daemon::Request;

/// Where a command runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route<'a> {
    /// Answered by the daemon over D-Bus.
    Daemon(Request<'a>),
    /// `stillwatch idle-test`, which runs the idle sources locally.
    IdleTest(&'a IdleTestArgs),
    /// `stillwatch config`, which works on the file directly.
    Config(&'a ConfigCommand),
}

/// Sorts `command` by where it runs.
#[must_use]
pub const fn route(command: &Command) -> Route<'_> {
    match command {
        Command::Status(args) => Route::Daemon(Request::Status(args)),
        Command::Snooze(args) => Route::Daemon(Request::Snooze(args)),
        Command::CancelSnooze => Route::Daemon(Request::CancelSnooze),
        Command::Pause => Route::Daemon(Request::Pause),
        Command::Resume => Route::Daemon(Request::Resume),
        Command::Reload => Route::Daemon(Request::Reload),
        Command::History(args) => Route::Daemon(Request::History(args)),
        Command::Probe(args) => Route::Daemon(Request::Probe(args)),
        Command::IdleTest(args) => Route::IdleTest(args),
        Command::Config { command } => Route::Config(command),
    }
}

/// Runs the command in `cli`, writing its output to `out`.
///
/// # Errors
///
/// Returns whatever the handler returns.
pub fn dispatch(cli: &Cli, style: &Style, out: &mut dyn Write) -> anyhow::Result<()> {
    match route(&cli.command) {
        Route::Daemon(request) => daemon::run(request, cli.bus_address.as_deref(), style, out),
        Route::IdleTest(args) => diagnostics::idle_test(args),
        Route::Config(ConfigCommand::Init(args)) => config::init(args, out),
        Route::Config(ConfigCommand::Check(args)) => config::check(args, out),
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser as _;
    use jiff::tz::TimeZone;

    use super::*;

    fn cli(args: &[&str]) -> Cli {
        Cli::try_parse_from(std::iter::once("stillwatch").chain(args.iter().copied())).unwrap()
    }

    #[test]
    fn daemon_commands_route_to_the_daemon() {
        for args in [
            &["status"][..],
            &["snooze", "45m"],
            &["cancel-snooze"],
            &["pause"],
            &["resume"],
            &["reload"],
            &["history"],
            &["probe"],
        ] {
            let cli = cli(args);
            assert!(matches!(route(&cli.command), Route::Daemon(_)), "{args:?}");
        }
        assert!(matches!(
            route(&cli(&["idle-test"]).command),
            Route::IdleTest(_)
        ));
        assert!(matches!(
            route(&cli(&["config", "check"]).command),
            Route::Config(ConfigCommand::Check(_))
        ));
    }

    #[test]
    fn dispatch_reaches_config_and_daemon_handlers() {
        let style = Style::plain(TimeZone::UTC);
        let mut out = Vec::new();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let path = path.to_str().unwrap();
        dispatch(&cli(&["config", "init", path]), &style, &mut out).unwrap();
        dispatch(&cli(&["config", "check", path]), &style, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text, format!("wrote {path}\n{path}: ok\n"));

        let bus = format!("unix:path={}", dir.path().join("no-bus").display());
        let err = dispatch(
            &cli(&["--bus-address", &bus, "pause"]),
            &style,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .starts_with("can't connect to the D-Bus session bus"),
            "{err}"
        );
    }
}
