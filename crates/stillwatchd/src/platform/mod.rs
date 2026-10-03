//! Probe the session once and pick backends from what is actually there.
//!
//! [`probe_session`] runs at startup and again after [`until_change`].
//! [`select`] is pure: the same probe and config always pick the same
//! backends. Capture globals for a later wlr backend are the
//! [`WlrCaptureGlobals`] extension point (JUS-48) and are not selected.

mod dbus;
mod facts;
mod grant;
mod select;
mod tools;
mod watch;
mod wayland;

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use stillwatch_core::backend::BackendError;
use wayland_client::Connection;

pub use facts::{
    DbusFacts, KwinBus, Probe, SessionServices, ToolFacts, WaylandFacts, WlrCaptureGlobals,
};
pub use select::{CaptureChoice, CaptureError, IdleError, Selection, select};
pub use watch::until_change;

const TIMEOUT: Duration = Duration::from_secs(5);

/// Where to look for grants, programs, and i2c nodes. Tests pass a sandbox.
#[derive(Debug, Clone)]
pub struct ProbeEnv {
    /// `applications` directories that may hold a `ScreenShot2` grant.
    pub app_dirs: Vec<PathBuf>,
    /// This process's executable, as `KWin` sees it.
    pub exe: PathBuf,
    /// `PATH` to search for `kscreen-doctor`.
    pub path: OsString,
    /// sysfs `i2c-dev` class directory.
    pub i2c_dev: PathBuf,
    /// sysfs DRM class directory.
    pub drm: PathBuf,
    /// Device directory (`/dev` on the running system).
    pub dev: PathBuf,
}

impl ProbeEnv {
    /// The process environment: XDG application dirs, `PATH`, and sysfs.
    #[must_use]
    pub fn system() -> Self {
        Self {
            app_dirs: grant::application_dirs(),
            exe: current_exe(),
            path: std::env::var_os("PATH").unwrap_or_default(),
            i2c_dev: crate::action::ddc::SYSFS_I2C_DEV.into(),
            drm: crate::action::ddc::SYSFS_DRM.into(),
            dev: PathBuf::from("/dev"),
        }
    }
}

/// Why a probe couldn't read the compositor.
#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    /// The Wayland connection failed or the registry didn't answer.
    #[error("platform probe: {0}")]
    Wayland(BackendError),
}

/// Probes the current session: `WAYLAND_DISPLAY`, the session bus, and the
/// system bus when it's there.
///
/// A missing session bus leaves the D-Bus facts empty and is not an error.
/// A missing compositor is.
///
/// # Errors
///
/// [`PlatformError::Wayland`] when the compositor can't be reached or doesn't
/// answer the registry.
pub async fn probe_session() -> Result<Probe, PlatformError> {
    let wayland = crate::wayland::connect_to(None).map_err(PlatformError::Wayland)?;
    let session = crate::dbus::connect(&crate::dbus::Bus::Session, TIMEOUT)
        .await
        .ok();
    let system = crate::dbus::connect(&crate::dbus::Bus::System, TIMEOUT)
        .await
        .ok();
    if session.is_none() {
        tracing::warn!("no session bus; D-Bus backends look unavailable");
    }
    probe_with(
        &wayland,
        session.as_ref(),
        system.as_ref(),
        &ProbeEnv::system(),
    )
    .await
}

/// Probes `wayland` and the buses the caller already opened.
///
/// # Errors
///
/// [`PlatformError::Wayland`] when the registry round trip fails.
pub async fn probe_with(
    wayland: &Connection,
    session: Option<&zbus::Connection>,
    system: Option<&zbus::Connection>,
    env: &ProbeEnv,
) -> Result<Probe, PlatformError> {
    let wayland = wayland.clone();
    let wayland_facts = tokio::task::spawn_blocking(move || wayland::read(&wayland))
        .await
        .map_err(|error| PlatformError::Wayland(BackendError::Disconnected(error.to_string())))?
        .map_err(PlatformError::Wayland)?;
    let mut dbus_facts = match session {
        Some(session) => dbus::read_session(session).await,
        None => DbusFacts::default(),
    };
    dbus_facts.kwin.screenshot2_authorized = grant::screenshot_authorized(&env.app_dirs, &env.exe);
    if let Some(system) = system {
        dbus_facts.session.logind = dbus::logind_present(system).await;
    }
    Ok(Probe {
        wayland: wayland_facts,
        dbus: dbus_facts,
        tools: tools::read(env),
    })
}

/// This process's executable, for the `ScreenShot2` grant check.
#[must_use]
pub fn current_exe() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("stillwatchd"))
}
