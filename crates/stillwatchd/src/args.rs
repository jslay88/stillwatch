//! Command-line arguments for `stillwatchd`.

use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;
use clap::builder::PossibleValuesParser;
use stillwatch_ipc::logging::LEVELS;
use stillwatch_ipc::paths::{self, PathsError};

/// Watches for idle input and static screens, then protects OLED panels.
#[derive(Debug, Clone, PartialEq, Eq, Parser)]
#[command(name = "stillwatchd", version)]
pub struct Args {
    /// Config file to load [default: `$XDG_CONFIG_HOME/stillwatch/config.toml`]
    #[arg(long, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// Log level; beats `RUST_LOG` and the config's `logging.level`
    #[arg(long, value_name = "LEVEL", value_parser = PossibleValuesParser::new(LEVELS))]
    pub log_level: Option<String>,

    /// Capture OUTPUT once with `KWin` `ScreenShot2`, print its size and
    /// format, and exit (checks the `.desktop` authorization)
    #[arg(long, value_name = "OUTPUT")]
    pub capture_check: Option<String>,

    /// Capture on an interval and print one `ProbeSample` JSON line per sample
    #[arg(long, conflicts_with = "capture_check")]
    pub probe: bool,

    /// Time between probe captures [default: `stale.check_interval_seconds`]
    #[arg(long, value_name = "DURATION", value_parser = parse_duration, requires = "probe")]
    pub interval: Option<Duration>,

    /// Stop the probe after this many samples [default: run until SIGTERM/SIGINT]
    #[arg(long, value_name = "N", requires = "probe")]
    pub count: Option<NonZeroUsize>,
}

fn parse_duration(text: &str) -> Result<Duration, String> {
    let parsed = humantime::parse_duration(text).map_err(|err| err.to_string())?;
    (parsed > Duration::ZERO)
        .then_some(parsed)
        .ok_or_else(|| "duration must be greater than zero".to_owned())
}

impl Args {
    /// The config file to use: `--config` if given, otherwise the default
    /// location.
    ///
    /// # Errors
    ///
    /// Fails if `--config` wasn't given and the config directory can't be
    /// resolved.
    pub fn config_path(&self) -> Result<PathBuf, PathsError> {
        self.config.clone().map_or_else(paths::config_file, Ok)
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;
    use std::path::Path;
    use std::time::Duration;

    use clap::CommandFactory;
    use clap::error::ErrorKind;

    use super::*;

    fn parse(args: &[&str]) -> Result<Args, clap::Error> {
        Args::try_parse_from(std::iter::once("stillwatchd").chain(args.iter().copied()))
    }

    #[test]
    fn no_args_uses_defaults() {
        let args = parse(&[]).unwrap();
        assert_eq!(
            args,
            Args {
                config: None,
                log_level: None,
                capture_check: None,
                probe: false,
                interval: None,
                count: None,
            }
        );
        assert_eq!(args.config_path(), paths::config_file());
    }

    #[test]
    fn config_flag_overrides_default_path() {
        let args = parse(&["--config", "/etc/stillwatch.toml"]).unwrap();
        assert_eq!(
            args.config_path().unwrap(),
            Path::new("/etc/stillwatch.toml")
        );
    }

    #[test]
    fn log_level_accepts_known_levels() {
        for level in LEVELS {
            let args = parse(&["--log-level", level]).unwrap();
            assert_eq!(args.log_level.as_deref(), Some(level));
        }
    }

    #[test]
    fn log_level_rejects_unknown_levels() {
        let err = parse(&["--log-level", "loud"]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidValue);
    }

    #[test]
    fn capture_check_takes_an_output() {
        let args = parse(&["--capture-check", "HDMI-A-1"]).unwrap();
        assert_eq!(args.capture_check.as_deref(), Some("HDMI-A-1"));
        let err = parse(&["--capture-check"]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidValue);
    }

    #[test]
    fn probe_takes_interval_and_count() {
        let args = parse(&["--probe", "--interval", "5s", "--count", "3"]).unwrap();
        assert!(args.probe);
        assert_eq!(args.interval, Some(Duration::from_secs(5)));
        assert_eq!(args.count, NonZeroUsize::new(3));
        let err = parse(&["--interval", "5s"]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::MissingRequiredArgument);
        let err = parse(&["--probe", "--interval", "0s"]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::ValueValidation);
        let err = parse(&["--probe", "--capture-check", "HDMI-A-1"]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::ArgumentConflict);
    }

    #[test]
    fn unknown_flags_are_rejected() {
        let err = parse(&["--daemonize"]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::UnknownArgument);
    }

    #[test]
    fn command_definition_is_valid() {
        Args::command().debug_assert();
    }
}
