//! Building `kscreen-doctor --dpms` commands and reading their results.
//!
//! kscreen-doctor (libkscreen 6.7.5) can only *exclude* outputs, with one
//! `--dpms-excluded <connector>` per output, matched against the Qt screen
//! name (the `wl_output` name). Two quirks shape this module:
//!
//! - If every screen is excluded, it switches *all* of them, so a request
//!   that would exclude everything is never sent.
//! - It reports "not on Wayland" and "DPMS not supported" on stderr and
//!   still exits 0.

use std::time::Duration;

use stillwatch_core::backend::BackendError;

use crate::process::{CommandOutput, CommandSpec, tail_of_stderr};

/// The program, looked up on `PATH`.
pub const PROGRAM: &str = "kscreen-doctor";

/// How long kscreen-doctor may take. It normally finishes well under a
/// second; it hangs on input it doesn't understand.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// Stderr lines that mean kscreen-doctor did nothing, though it exited 0.
const SILENT_FAILURES: [&str; 2] = ["DPMS is only supported on Wayland", "DPMS not supported"];

/// The DPMS state to ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Power {
    /// Signal off, so the display can reach standby.
    Off,
    /// Displays back on.
    On,
}

impl Power {
    const fn arg(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::On => "on",
        }
    }
}

/// Which connected outputs to exclude so only `targets` switch, or `None`
/// when there's nothing to do.
///
/// Targets that aren't connected are skipped with a warning. When none of
/// them is connected, blanking fails with [`BackendError::NotFound`] (the
/// action didn't happen) while unblanking is a no-op (nothing to wake).
///
/// # Errors
///
/// [`BackendError::NotFound`] as above.
pub fn exclusions(
    power: Power,
    targets: &[String],
    connected: &[String],
) -> Result<Option<Vec<String>>, BackendError> {
    let missing: Vec<&String> = targets.iter().filter(|t| !connected.contains(t)).collect();
    if missing.len() == targets.len() {
        return match power {
            Power::Off => Err(BackendError::NotFound(format!(
                "none of the outputs to blank are connected: {}",
                targets.join(", ")
            ))),
            Power::On => Ok(None),
        };
    }
    if !missing.is_empty() {
        tracing::warn!(
            ?missing,
            ?connected,
            "skipping outputs that aren't connected"
        );
    }
    Ok(Some(
        connected
            .iter()
            .filter(|output| !targets.contains(output))
            .cloned()
            .collect(),
    ))
}

/// Where and how to run kscreen-doctor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    /// The program name or path.
    pub program: String,
    /// Kill it after this long.
    pub timeout: Duration,
    /// The Wayland display to talk to, or `None` for the daemon's own.
    pub display: Option<String>,
}

impl Default for Invocation {
    fn default() -> Self {
        Self {
            program: PROGRAM.to_owned(),
            timeout: DEFAULT_TIMEOUT,
            display: None,
        }
    }
}

impl Invocation {
    /// The command switching every output except `excluded` to `power`.
    ///
    /// Qt is pinned to its Wayland platform: on any other, kscreen-doctor
    /// refuses DPMS, and it must never fall back to an X11 display.
    #[must_use]
    pub fn command(&self, power: Power, excluded: &[String]) -> CommandSpec {
        let mut spec = CommandSpec::new(&self.program, self.timeout)
            .args(["--dpms", power.arg()])
            .env("QT_QPA_PLATFORM", "wayland");
        for output in excluded {
            spec = spec.arg("--dpms-excluded").arg(output);
        }
        if let Some(display) = &self.display {
            spec = spec
                .env("WAYLAND_DISPLAY", display)
                .env_remove("WAYLAND_SOCKET");
        }
        spec
    }
}

/// Turns a run that exited 0 but reported a refusal into an error.
///
/// # Errors
///
/// [`BackendError::Unsupported`] with kscreen-doctor's message.
pub fn check(output: &CommandOutput) -> Result<(), BackendError> {
    if SILENT_FAILURES.iter().any(|m| output.stderr.contains(m)) {
        return Err(BackendError::Unsupported(format!(
            "{PROGRAM}: {}",
            tail_of_stderr(&output.stderr)
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(names: &[&str]) -> Vec<String> {
        names.iter().map(|&n| n.to_owned()).collect()
    }

    #[test]
    fn excludes_every_connected_output_outside_the_targets() {
        let connected = names(&["DP-1", "HDMI-A-1", "DP-2"]);
        assert_eq!(
            exclusions(Power::Off, &names(&["HDMI-A-1"]), &connected),
            Ok(Some(names(&["DP-1", "DP-2"])))
        );
        assert_eq!(
            exclusions(Power::On, &names(&["DP-2", "DP-1"]), &connected),
            Ok(Some(names(&["HDMI-A-1"])))
        );
        assert_eq!(
            exclusions(Power::Off, &connected, &connected),
            Ok(Some(Vec::new()))
        );
    }

    #[test]
    fn unconnected_targets_are_skipped() {
        let connected = names(&["DP-1", "HDMI-A-1"]);
        assert_eq!(
            exclusions(Power::Off, &names(&["HDMI-A-1", "DP-9"]), &connected),
            Ok(Some(names(&["DP-1"])))
        );
    }

    #[test]
    fn nothing_connected_fails_a_blank_and_skips_an_unblank() {
        let connected = names(&["DP-1"]);
        let gone = names(&["HDMI-A-1"]);
        let error = exclusions(Power::Off, &gone, &connected).unwrap_err();
        assert!(
            matches!(&error, BackendError::NotFound(m) if m.contains("HDMI-A-1")),
            "{error}"
        );
        assert_eq!(exclusions(Power::On, &gone, &connected), Ok(None));
        assert_eq!(exclusions(Power::On, &gone, &[]), Ok(None));
    }

    #[test]
    fn builds_one_exclusion_flag_per_output() {
        let spec = Invocation::default().command(Power::Off, &names(&["DP-1", "DP-2"]));
        assert_eq!(spec.program, "kscreen-doctor");
        assert_eq!(
            spec.args,
            [
                "--dpms",
                "off",
                "--dpms-excluded",
                "DP-1",
                "--dpms-excluded",
                "DP-2"
            ]
        );
        assert_eq!(spec.env, [("QT_QPA_PLATFORM".into(), "wayland".into())]);
        assert_eq!(spec.env_remove, Vec::<String>::new());
        assert_eq!(spec.timeout, DEFAULT_TIMEOUT);
    }

    #[test]
    fn a_named_display_is_passed_to_the_child() {
        let invocation = Invocation {
            program: "/opt/kscreen-doctor".into(),
            timeout: Duration::from_secs(2),
            display: Some("stillwatch-test-1".into()),
        };
        let spec = invocation.command(Power::On, &[]);
        assert_eq!(spec.to_string(), "/opt/kscreen-doctor --dpms on");
        assert_eq!(
            spec.env,
            [
                ("QT_QPA_PLATFORM".into(), "wayland".into()),
                ("WAYLAND_DISPLAY".into(), "stillwatch-test-1".into()),
            ]
        );
        assert_eq!(spec.env_remove, ["WAYLAND_SOCKET"]);
        assert_eq!(spec.timeout, Duration::from_secs(2));
    }

    #[test]
    fn refusals_on_stderr_are_unsupported_even_with_exit_zero() {
        for stderr in [
            "DPMS is only supported on Wayland.\n",
            "qt.qpa: noise\nDPMS not supported in this system",
        ] {
            let output = CommandOutput {
                stdout: String::new(),
                stderr: stderr.into(),
            };
            let error = check(&output).unwrap_err();
            assert!(
                matches!(&error, BackendError::Unsupported(m) if m.starts_with("kscreen-doctor: ")),
                "{error}"
            );
        }
        let noisy = CommandOutput {
            stdout: String::new(),
            stderr: "kf.windowsystem: something unrelated".into(),
        };
        assert_eq!(check(&noisy), Ok(()));
    }
}
