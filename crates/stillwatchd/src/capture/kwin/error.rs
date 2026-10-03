//! Maps `CaptureScreen` failures to [`BackendError`], with remediation for
//! the one users can fix themselves: authorization.
//!
//! Error names are from `KWin` 6.7.5 `screenshotdbusinterface2.cpp`.

use std::io::ErrorKind;
use std::path::Path;

use stillwatch_core::backend::BackendError;
use zbus::DBusError as _;

/// The `.desktop` file that authorizes `stillwatchd` for `ScreenShot2`.
pub const DESKTOP_FILE: &str = "io.github.jslay88.Stillwatch.Daemon.desktop";

const NOT_AUTHORIZED: &str = "org.kde.KWin.ScreenShot2.Error.NoAuthorized";
const INVALID_SCREEN: &str = "org.kde.KWin.ScreenShot2.Error.InvalidScreen";
const CANCELLED: &str = "org.kde.KWin.ScreenShot2.Error.Cancelled";
const FILE_DESCRIPTOR: &str = "org.kde.KWin.ScreenShot2.Error.FileDescriptor";
const ACCESS_DENIED: &str = "org.freedesktop.DBus.Error.AccessDenied";
const MISSING: [&str; 5] = [
    "org.freedesktop.DBus.Error.ServiceUnknown",
    "org.freedesktop.DBus.Error.NameHasNoOwner",
    "org.freedesktop.DBus.Error.UnknownObject",
    "org.freedesktop.DBus.Error.UnknownInterface",
    "org.freedesktop.DBus.Error.UnknownMethod",
];

/// Maps a failed `CaptureScreen` (or `Version`) call for `output`. `exe` is
/// this process's executable, as `KWin` sees it in `/proc/<pid>/exe`.
#[must_use]
pub fn call_error(error: zbus::Error, output: &str, exe: &Path) -> BackendError {
    match error {
        zbus::Error::MethodError(name, description, _) => {
            method_error(name.as_str(), description.as_deref(), output, exe)
        }
        zbus::Error::FDO(fdo) => method_error(fdo.name().as_str(), fdo.description(), output, exe),
        zbus::Error::InterfaceNotFound => {
            BackendError::Unavailable("KWin ScreenShot2 isn't available".into())
        }
        zbus::Error::InputOutput(io) if io.kind() == ErrorKind::TimedOut => {
            BackendError::Io(format!("KWin didn't answer the ScreenShot2 call: {io}"))
        }
        zbus::Error::InputOutput(io) => {
            BackendError::Disconnected(format!("session bus connection lost: {io}"))
        }
        other => BackendError::Protocol(format!("ScreenShot2 call failed: {other}")),
    }
}

/// Maps a D-Bus error reply by name.
#[must_use]
pub fn method_error(
    name: &str,
    description: Option<&str>,
    output: &str,
    exe: &Path,
) -> BackendError {
    let detail = description.unwrap_or("no details");
    match name {
        NOT_AUTHORIZED | ACCESS_DENIED => BackendError::PermissionDenied(not_authorized(exe)),
        INVALID_SCREEN => BackendError::NotFound(format!("KWin has no output named {output:?}")),
        CANCELLED => BackendError::Io(format!(
            "KWin couldn't render {output:?} for the screenshot (it needs OpenGL compositing): {detail}"
        )),
        FILE_DESCRIPTOR => {
            BackendError::Io(format!("KWin couldn't use the capture pipe: {detail}"))
        }
        missing if MISSING.contains(&missing) => {
            BackendError::Unavailable(format!("KWin ScreenShot2 isn't available: {detail}"))
        }
        other => BackendError::Protocol(format!("ScreenShot2 failed with {other}: {detail}")),
    }
}

/// What to tell the user when `KWin` refuses the capture.
///
/// `KWin` reads the caller's `/proc/<pid>/exe` and looks for an installed
/// application `.desktop` file whose first `Exec=` word resolves (symlinks
/// followed) to exactly that path and whose `X-KDE-DBUS-Restricted-Interfaces`
/// lists `org.kde.KWin.ScreenShot2`. Its service cache is rebuilt when an
/// applications directory's mtime changes, which an in-place edit doesn't do.
#[must_use]
pub fn not_authorized(exe: &Path) -> String {
    let shown = exe.display().to_string();
    if let Some(original) = shown.strip_suffix(" (deleted)") {
        return format!(
            "KWin refused the screenshot because {original} was replaced after stillwatchd \
             started, so KWin can't match the running process to a .desktop file. Restart \
             stillwatchd"
        );
    }
    format!(
        "KWin only lets authorized programs use org.kde.KWin.ScreenShot2. Install {DESKTOP_FILE} \
         into ~/.local/share/applications/ (or /usr/share/applications/) with Exec={shown} and \
         X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2. Exec must be an absolute \
         path to this binary (symlinks are fine); the file name doesn't matter. KWin doesn't \
         notice edits to an installed file until its directory changes, so after editing one, \
         touch the directory"
    )
}

#[cfg(test)]
mod tests;
