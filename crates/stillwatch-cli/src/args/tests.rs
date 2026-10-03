use clap::CommandFactory;
use clap::error::ErrorKind;

use super::*;

fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
    Cli::try_parse_from(std::iter::once("stillwatch").chain(args.iter().copied()))
}

fn command(args: &[&str]) -> Command {
    parse(args).unwrap().command
}

fn minutes(n: u64) -> Duration {
    Duration::from_mins(n)
}

#[test]
fn command_definition_is_valid() {
    Cli::command().debug_assert();
}

#[test]
fn status() {
    assert_eq!(
        command(&["status"]),
        Command::Status(StatusArgs { json: false })
    );
    assert_eq!(
        command(&["status", "--json"]),
        Command::Status(StatusArgs { json: true })
    );
}

#[test]
fn snooze_parses_duration() {
    assert_eq!(
        command(&["snooze", "45m"]),
        Command::Snooze(SnoozeArgs {
            duration: minutes(45)
        })
    );
    assert_eq!(
        command(&["snooze", "1h30m"]),
        Command::Snooze(SnoozeArgs {
            duration: minutes(90)
        })
    );
}

#[test]
fn snooze_requires_duration() {
    let err = parse(&["snooze"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::MissingRequiredArgument);
}

#[test]
fn snooze_rejects_invalid_durations() {
    for bad in ["soon", "45", "5 parsecs", "0m"] {
        let err = parse(&["snooze", bad]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::ValueValidation, "{bad}");
        assert!(err.to_string().contains(bad), "{bad}: {err}");
    }
}

#[test]
fn zero_snooze_explains_why() {
    let err = parse(&["snooze", "0s"]).unwrap_err();
    assert!(err.to_string().contains("greater than zero"), "{err}");
}

#[test]
fn simple_commands() {
    assert_eq!(command(&["cancel-snooze"]), Command::CancelSnooze);
    assert_eq!(command(&["pause"]), Command::Pause);
    assert_eq!(command(&["resume"]), Command::Resume);
    assert_eq!(command(&["reload"]), Command::Reload);
}

#[test]
fn simple_commands_take_no_arguments() {
    for name in ["cancel-snooze", "pause", "resume", "reload"] {
        let err = parse(&[name, "extra"]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::UnknownArgument, "{name}");
    }
}

#[test]
fn history_defaults() {
    assert_eq!(
        command(&["history"]),
        Command::History(HistoryArgs {
            since: None,
            json: false
        })
    );
}

#[test]
fn history_with_since_and_json() {
    assert_eq!(
        command(&["history", "--since", "2h", "--json"]),
        Command::History(HistoryArgs {
            since: Some(minutes(120)),
            json: true
        })
    );
}

#[test]
fn history_rejects_invalid_since() {
    let err = parse(&["history", "--since", "yesterday"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::ValueValidation);
}

#[test]
fn probe() {
    assert_eq!(
        command(&["probe"]),
        Command::Probe(ProbeArgs { interval: None })
    );
    assert_eq!(
        command(&["probe", "--interval", "10s"]),
        Command::Probe(ProbeArgs {
            interval: Some(Duration::from_secs(10))
        })
    );
    let err = parse(&["probe", "--interval", "0s"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::ValueValidation);
}

#[test]
fn idle_test() {
    assert_eq!(
        command(&["idle-test"]),
        Command::IdleTest(IdleTestArgs { timeout: None })
    );
    assert_eq!(
        command(&["idle-test", "--timeout", "30s"]),
        Command::IdleTest(IdleTestArgs {
            timeout: Some(Duration::from_secs(30))
        })
    );
    let err = parse(&["idle-test", "--timeout", "fast"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::ValueValidation);
}

#[test]
fn idle_test_takes_minutes_as_a_bare_number() {
    for args in [
        ["idle-test", "--minutes", "1"],
        ["idle-test", "--timeout", "1"],
        ["idle-test", "--minutes", "60s"],
    ] {
        assert_eq!(
            command(&args),
            Command::IdleTest(IdleTestArgs {
                timeout: Some(minutes(1))
            }),
            "{args:?}"
        );
    }
    let err = parse(&["idle-test", "--minutes", "0"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::ValueValidation);
    let err = parse(&["idle-test", "--minutes", "1", "--timeout", "2"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::ArgumentConflict);
}

#[test]
fn config_init() {
    assert_eq!(
        command(&["config", "init"]),
        Command::Config {
            command: ConfigCommand::Init(ConfigInitArgs {
                force: false,
                path: None
            })
        }
    );
    assert_eq!(
        command(&["config", "init", "--force", "/tmp/sw.toml"]),
        Command::Config {
            command: ConfigCommand::Init(ConfigInitArgs {
                force: true,
                path: Some(PathBuf::from("/tmp/sw.toml")),
            })
        }
    );
}

#[test]
fn config_check() {
    assert_eq!(
        command(&["config", "check"]),
        Command::Config {
            command: ConfigCommand::Check(ConfigCheckArgs { path: None })
        }
    );
    assert_eq!(
        command(&["config", "check", "./config.toml"]),
        Command::Config {
            command: ConfigCommand::Check(ConfigCheckArgs {
                path: Some(PathBuf::from("./config.toml")),
            })
        }
    );
}

#[test]
fn config_requires_an_action() {
    let err = parse(&["config"]).unwrap_err();
    assert_eq!(
        err.kind(),
        ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
    );
}

#[test]
fn subcommand_is_required() {
    let err = parse(&[]).unwrap_err();
    assert_eq!(
        err.kind(),
        ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
    );
}

#[test]
fn unknown_subcommand_is_rejected() {
    let err = parse(&["blank"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidSubcommand);
}

#[test]
fn log_level_is_global() {
    let cli = parse(&["pause", "--log-level", "debug"]).unwrap();
    assert_eq!(cli.log_level.as_deref(), Some("debug"));
    let cli = parse(&["--log-level", "trace", "status"]).unwrap();
    assert_eq!(cli.log_level.as_deref(), Some("trace"));
    let err = parse(&["status", "--log-level", "chatty"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidValue);
}

#[test]
fn help_lists_every_subcommand() {
    let help = Cli::command().render_help().to_string();
    for name in [
        "status",
        "snooze",
        "cancel-snooze",
        "pause",
        "resume",
        "reload",
        "history",
        "probe",
        "idle-test",
        "config",
    ] {
        assert!(help.contains(name), "{name} missing from:\n{help}");
    }
}

#[test]
fn every_subcommand_has_help_text() {
    fn check(command: &clap::Command) {
        for sub in command.get_subcommands() {
            assert!(sub.get_about().is_some(), "{} has no help", sub.get_name());
            check(sub);
        }
    }
    check(&Cli::command());
}
