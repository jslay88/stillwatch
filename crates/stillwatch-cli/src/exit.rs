//! Exit codes, the same for every command.
//!
//! | Code | Meaning |
//! | -- | -- |
//! | 0 | success |
//! | 1 | the command failed: the daemon refused it, the config is invalid, or anything else went wrong |
//! | 2 | usage error (bad flags or arguments) |
//! | 3 | stillwatchd isn't running |

use std::io::{self, Write};

use crate::connect::DaemonError;

/// Success.
pub const OK: u8 = 0;
/// The command failed; the message says why.
pub const FAILED: u8 = 1;
/// Bad flags or arguments (what clap exits with).
pub const USAGE: u8 = 2;
/// The daemon isn't running.
pub const NOT_RUNNING: u8 = 3;

/// The exit code for a failed command.
#[must_use]
pub fn code(err: &anyhow::Error) -> u8 {
    let not_running = err
        .chain()
        .filter_map(|cause| cause.downcast_ref::<DaemonError>())
        .any(DaemonError::is_not_running);
    if not_running { NOT_RUNNING } else { FAILED }
}

/// Prints a failure to `stderr` and returns the exit code for `result`.
///
/// A closed stdout (as in `stillwatch probe | head`) counts as success.
pub fn report(result: &anyhow::Result<()>, stderr: &mut dyn Write) -> u8 {
    let Err(err) = result else {
        return OK;
    };
    if is_broken_pipe(err) {
        return OK;
    }
    // Nothing useful is left to do if stderr is gone too.
    let _ = writeln!(stderr, "error: {err:#}");
    code(err)
}

fn is_broken_pipe(err: &anyhow::Error) -> bool {
    err.chain()
        .filter_map(|cause| cause.downcast_ref::<io::Error>())
        .any(|io| io.kind() == io::ErrorKind::BrokenPipe)
}

#[cfg(test)]
mod tests {
    use anyhow::Context as _;
    use clap::Parser as _;

    use super::*;
    use crate::Cli;

    fn reported(result: &anyhow::Result<()>) -> (u8, String) {
        let mut stderr = Vec::new();
        let code = report(result, &mut stderr);
        (code, String::from_utf8(stderr).unwrap())
    }

    #[test]
    fn success_prints_nothing() {
        assert_eq!(reported(&Ok(())), (OK, String::new()));
    }

    #[test]
    fn not_running_is_three_even_with_context() {
        let err = Err(DaemonError::NotRunning).context("status");
        let (exit, stderr) = reported(&err);
        assert_eq!(exit, NOT_RUNNING);
        assert_eq!(
            stderr,
            "error: status: stillwatchd is not running; \
             start it with systemctl --user start stillwatch\n"
        );
        assert_eq!(code(&DaemonError::Stopped.into()), NOT_RUNNING);
    }

    #[test]
    fn refusals_and_other_failures_are_one() {
        let refused = anyhow::Error::from(DaemonError::Refused("too long".into()));
        assert_eq!(
            reported(&Err(refused)),
            (FAILED, "error: too long\n".into())
        );
        assert_eq!(code(&anyhow::anyhow!("config is invalid")), FAILED);
    }

    #[test]
    fn a_closed_stdout_is_not_an_error() {
        let err = anyhow::Error::from(io::Error::from(io::ErrorKind::BrokenPipe));
        assert_eq!(reported(&Err(err.context("writing"))), (OK, String::new()));
        let other = anyhow::Error::from(io::Error::from(io::ErrorKind::PermissionDenied));
        assert_eq!(reported(&Err(other)).0, FAILED);
    }

    #[test]
    fn usage_matches_clap() {
        let err = Cli::try_parse_from(["stillwatch", "bogus"]).unwrap_err();
        assert_eq!(err.exit_code(), i32::from(USAGE));
    }
}
