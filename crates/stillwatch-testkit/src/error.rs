use std::path::PathBuf;
use std::process::ExitStatus;

use crate::bus::REQUIRE_ENV;
use crate::kwin::REQUIRE_ENV as REQUIRE_KWIN_ENV;

/// Why a test helper couldn't start or drive its bus.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The bus daemon isn't installed and the environment requires it.
    #[error("{0} isn't installed, and {REQUIRE_ENV}=1 requires it")]
    Missing(String),
    /// The bus daemon exited or closed stdout before printing its address.
    #[error("dbus-daemon didn't print a bus address")]
    NoAddress,
    /// Spawning or talking to the bus daemon process failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// A D-Bus call or connection failed.
    #[error(transparent)]
    Bus(#[from] zbus::Error),
    /// `kwin_wayland` isn't installed and the environment requires it.
    #[error("{0} isn't installed, and {REQUIRE_KWIN_ENV}=1 requires it")]
    KwinMissing(String),
    /// `KWin` exited before it was ready.
    #[error("kwin_wayland exited during startup ({status}). Its log:\n{log}")]
    KwinExited {
        /// How it exited.
        status: ExitStatus,
        /// The end of its stdout and stderr.
        log: String,
    },
    /// Something didn't become ready before its deadline.
    #[error("timed out waiting for {what}. KWin's log:\n{log}")]
    Timeout {
        /// What was being waited for.
        what: String,
        /// The end of `KWin`'s stdout and stderr.
        log: String,
    },
    /// Talking to the Wayland compositor failed.
    #[error("Wayland: {0}")]
    Wayland(String),
    /// A path can't go into a `.desktop` file's `Exec=` line unquoted.
    #[error("{} has characters that need quoting in Exec=", .0.display())]
    UnquotablePath(PathBuf),
}
