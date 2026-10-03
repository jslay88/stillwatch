//! Portal and `PipeWire` failures as [`BackendError`](stillwatch_core::backend::BackendError).
//!
//! A cancelled or missing portal is permanent for this process: retrying would
//! pop the permission dialog again, or fail the same way. A dropped session
//! or a `PipeWire` error is transient and goes through back-off.

use stillwatch_core::backend::BackendError;

use crate::dbus;

/// Whether `error` should stop portal capture until the daemon restarts.
///
/// Denial and a missing portal degrade to input-idle-only. A disconnect can
/// be retried.
#[must_use]
pub fn blocks_capture(error: &BackendError) -> bool {
    matches!(
        error,
        BackendError::PermissionDenied(_) | BackendError::Unavailable(_)
    )
}

/// Maps an `ashpd` failure.
#[must_use]
pub fn map_ashpd(error: ashpd::Error) -> BackendError {
    match error {
        ashpd::Error::Response(ashpd::desktop::ResponseError::Cancelled) => cancelled(),
        ashpd::Error::Response(ashpd::desktop::ResponseError::Other) => dismissed(),
        ashpd::Error::Portal(portal) => map_portal(portal),
        ashpd::Error::PortalNotFound(name) => BackendError::Unavailable(format!(
            "{name} isn't available, so capture falls back to input idle"
        )),
        ashpd::Error::Zbus(error) => dbus::call_error("xdg-desktop-portal ScreenCast", error),
        ashpd::Error::NoResponse => BackendError::Disconnected(
            "the portal closed the screen cast request without answering".into(),
        ),
        ashpd::Error::RequiresVersion(need, have) => BackendError::Unsupported(format!(
            "the portal is version {have}; Stillwatch needs {need}"
        )),
        ashpd::Error::IO(error) => BackendError::Io(error.to_string()),
        other => BackendError::Protocol(other.to_string()),
    }
}

/// Maps a `PipeWire` failure. These are retried after back-off.
#[must_use]
pub fn map_pipewire(error: &pipewire::Error) -> BackendError {
    BackendError::Disconnected(format!("PipeWire: {error}"))
}

fn map_portal(error: ashpd::PortalError) -> BackendError {
    match error {
        ashpd::PortalError::Cancelled(_) => cancelled(),
        ashpd::PortalError::NotAllowed(detail) => {
            BackendError::PermissionDenied(format!("the portal refused screen capture: {detail}"))
        }
        ashpd::PortalError::NotFound(detail) => BackendError::NotFound(detail),
        ashpd::PortalError::ZBus(error) => dbus::call_error("xdg-desktop-portal ScreenCast", error),
        other => BackendError::Disconnected(format!("portal screen cast failed: {other}")),
    }
}

fn cancelled() -> BackendError {
    BackendError::PermissionDenied(
        "screen sharing was cancelled, so capture falls back to input idle".into(),
    )
}

fn dismissed() -> BackendError {
    BackendError::PermissionDenied(
        "screen sharing was dismissed, so capture falls back to input idle".into(),
    )
}

#[cfg(test)]
mod tests;
