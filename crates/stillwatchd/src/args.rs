//! Command-line arguments for `stillwatchd`.

use std::path::PathBuf;

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
    use std::path::Path;

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
    fn unknown_flags_are_rejected() {
        let err = parse(&["--daemonize"]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::UnknownArgument);
    }

    #[test]
    fn command_definition_is_valid() {
        Args::command().debug_assert();
    }
}
