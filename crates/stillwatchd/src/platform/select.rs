//! Backend choice from a [`Probe`](super::Probe) and the config.
//!
//! `prompt.style = "auto"` stays on the notification when a notification
//! server is present. Fullscreen and Do Not Disturb are not inputs.
//! [`crate::prompt::NOTIFICATIONS_HIDDEN_OVER_FULLSCREEN`] stays false, so
//! this does not switch `auto` to the dialog for a fullscreen surface.

#[cfg(test)]
mod tests;

use std::path::Path;

use stillwatch_core::command::BlankMethod;
use stillwatch_core::config::{CaptureBackend, Config, PromptStyle};
use stillwatch_core::history::{HistoryEntry, HistoryKind, PromptMedium};
use stillwatch_ipc::status::BackendReport;

use super::facts::Probe;
use crate::capture::kwin::error::not_authorized;
use crate::idle::{MIN_NOTIFIER_VERSION, V1_ONLY};

/// Why idle can't start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum IdleError {
    /// The compositor doesn't advertise `ext_idle_notifier_v1`.
    #[error(
        "compositor doesn't advertise ext_idle_notifier_v1; input idle that ignores inhibitors is required"
    )]
    Missing,
    /// Only the inhibitor-respecting v1 is advertised.
    #[error("{V1_ONLY}")]
    V1Only,
}

/// Why a forced capture backend can't be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CaptureError {
    /// `capture.backend = "kwin"` and `ScreenShot2` isn't on the bus.
    #[error("capture.backend is \"kwin\", but org.kde.KWin.ScreenShot2 isn't available")]
    KwinMissing,
    /// `capture.backend = "kwin"` and this process has no `.desktop` grant for `ScreenShot2`.
    #[error(
        "capture.backend is \"kwin\", but this process isn't authorized for org.kde.KWin.ScreenShot2"
    )]
    KwinUnauthorized,
    /// `capture.backend = "portal"` and `ScreenCast` isn't on the bus.
    #[error("capture.backend is \"portal\", but org.freedesktop.portal.ScreenCast isn't available")]
    PortalMissing,
}

/// Which capture backend to open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureChoice {
    /// `org.kde.KWin.ScreenShot2`. `version` is the interface revision.
    Kwin {
        /// `Version` property, when the probe read it.
        version: Option<u32>,
    },
    /// xdg-desktop-portal `ScreenCast`.
    Portal,
    /// No capture. The daemon keeps running on input idle.
    InputIdleOnly,
    /// A forced backend isn't available. Startup treats this as an error.
    Unavailable,
}

/// The blank method to use, and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlankChoice {
    /// Method the daemon should blank with.
    pub method: BlankMethod,
    /// Short reason, for status and the startup log.
    pub reason: &'static str,
    /// `method` replaced the configured one.
    pub fell_back: bool,
}

/// Notification or dialog, and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PromptChoice {
    /// What will be shown.
    pub medium: PromptMedium,
    /// Short reason, for status and the startup log.
    pub reason: &'static str,
}

/// The backends a probe and a config pick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    /// Advertised notifier version when it is v2 or newer.
    pub idle_version: Option<u32>,
    /// Set when idle can't start.
    pub idle_error: Option<IdleError>,
    /// Capture backend to open.
    pub capture: CaptureChoice,
    /// Why that capture backend was picked.
    pub capture_reason: &'static str,
    /// Set when a forced capture backend isn't available.
    pub capture_error: Option<CaptureError>,
    /// Blank method, possibly an overlay fallback.
    pub blank: BlankChoice,
    /// Prompt medium.
    pub prompt: PromptChoice,
}

impl Selection {
    /// `kwin`, `portal`, or `None` for input-idle-only and a failed force.
    #[must_use]
    pub fn capture_backend_name(&self) -> Option<String> {
        match self.capture {
            CaptureChoice::Kwin { .. } => Some("kwin".to_owned()),
            CaptureChoice::Portal => Some("portal".to_owned()),
            CaptureChoice::InputIdleOnly | CaptureChoice::Unavailable => None,
        }
    }

    /// Wire name of the capture choice.
    #[must_use]
    pub const fn capture_name(&self) -> &'static str {
        match self.capture {
            CaptureChoice::Kwin { .. } => "kwin",
            CaptureChoice::Portal => "portal",
            CaptureChoice::InputIdleOnly => "input-idle-only",
            CaptureChoice::Unavailable => "unavailable",
        }
    }

    /// Idle label for status. A missing notifier says so.
    #[must_use]
    pub fn idle_label(&self) -> String {
        match self.idle_error {
            Some(IdleError::Missing) => "ext-idle-notify missing".to_owned(),
            Some(IdleError::V1Only) => "ext-idle-notify v1".to_owned(),
            None => match self.idle_version {
                Some(version) => format!("ext-idle-notify v{version}"),
                None => "ext-idle-notify missing".to_owned(),
            },
        }
    }

    /// Names only, for a history entry: idle, capture, blank, prompt.
    #[must_use]
    pub fn history_names(&self) -> String {
        let idle = match self.idle_error {
            Some(IdleError::Missing) => "idle-missing".to_owned(),
            Some(IdleError::V1Only) => "idle-v1".to_owned(),
            None => match self.idle_version {
                Some(version) => format!("ext-idle-notify-v{version}"),
                None => "idle-missing".to_owned(),
            },
        };
        format!(
            "{idle} {} {} {}",
            self.capture_name(),
            self.blank.method.as_str(),
            self.prompt.medium.as_str()
        )
    }

    /// Status block.
    #[must_use]
    pub fn report(&self) -> BackendReport {
        BackendReport {
            idle: self.idle_label(),
            capture: self.capture_name().to_owned(),
            capture_reason: self.capture_reason.to_owned(),
            blank: self.blank.method.as_str().to_owned(),
            blank_reason: self.blank.reason.to_owned(),
            prompt: self.prompt.medium.as_str().to_owned(),
            prompt_reason: self.prompt.reason.to_owned(),
        }
    }

    /// History entry for this choice. Names only, no pixels.
    #[must_use]
    pub fn history_entry(&self, at: jiff::Timestamp) -> HistoryEntry {
        HistoryEntry::new(at, HistoryKind::Backends).with_backends(self.history_names())
    }

    /// Why startup should stop, if idle or a forced capture backend failed.
    ///
    /// An unauthorized `KWin` force includes the `.desktop` remediation.
    #[must_use]
    pub fn startup_failure(&self, exe: &Path) -> Option<String> {
        if let Some(error) = self.idle_error {
            return Some(error.to_string());
        }
        match self.capture_error {
            Some(CaptureError::KwinUnauthorized) => Some(format!(
                "{}\n{}",
                CaptureError::KwinUnauthorized,
                not_authorized(exe)
            )),
            Some(error) => Some(error.to_string()),
            None => None,
        }
    }

    /// Blank method that replaces the configured one, when selection fell back.
    #[must_use]
    pub const fn blank_override(&self) -> Option<BlankMethod> {
        if self.blank.fell_back {
            Some(self.blank.method)
        } else {
            None
        }
    }

    /// One startup line: the choice and why.
    pub fn log_selected(&self) {
        tracing::info!(
            idle = %self.idle_label(),
            capture = self.capture_name(),
            capture_why = self.capture_reason,
            blank = self.blank.method.as_str(),
            blank_why = self.blank.reason,
            prompt = self.prompt.medium.as_str(),
            prompt_why = self.prompt.reason,
            "selected backends"
        );
    }
}

/// Picks backends. Does not open them.
#[must_use]
pub fn select(probe: &Probe, config: &Config) -> Selection {
    let (idle_version, idle_error) = select_idle(probe);
    let (capture, capture_reason, capture_error) = select_capture(probe, config.capture.backend);
    Selection {
        idle_version,
        idle_error,
        capture,
        capture_reason,
        capture_error,
        blank: select_blank(probe, config.action.blank_method),
        prompt: select_prompt(probe, config.prompt.style),
    }
}

fn select_idle(probe: &Probe) -> (Option<u32>, Option<IdleError>) {
    match probe.wayland.idle_notifier_version {
        Some(version) if version >= MIN_NOTIFIER_VERSION => (Some(version), None),
        Some(_) => (None, Some(IdleError::V1Only)),
        None => (None, Some(IdleError::Missing)),
    }
}

fn kwin_ready(probe: &Probe) -> bool {
    probe.dbus.kwin.present && probe.dbus.kwin.screenshot2 && probe.dbus.kwin.screenshot2_authorized
}

fn select_capture(
    probe: &Probe,
    backend: CaptureBackend,
) -> (CaptureChoice, &'static str, Option<CaptureError>) {
    let ready = kwin_ready(probe);
    let portal = probe.dbus.portal_screencast;
    let version = probe.dbus.kwin.screenshot2_version;
    match backend {
        CaptureBackend::Kwin if ready => (CaptureChoice::Kwin { version }, WHY_KWIN_FORCED, None),
        CaptureBackend::Kwin if !probe.dbus.kwin.screenshot2 || !probe.dbus.kwin.present => (
            CaptureChoice::Unavailable,
            WHY_KWIN_MISSING,
            Some(CaptureError::KwinMissing),
        ),
        CaptureBackend::Kwin => (
            CaptureChoice::Unavailable,
            WHY_KWIN_UNAUTHORIZED,
            Some(CaptureError::KwinUnauthorized),
        ),
        CaptureBackend::Portal if portal => (CaptureChoice::Portal, WHY_PORTAL_FORCED, None),
        CaptureBackend::Portal => (
            CaptureChoice::Unavailable,
            WHY_PORTAL_MISSING,
            Some(CaptureError::PortalMissing),
        ),
        CaptureBackend::Auto if ready => (CaptureChoice::Kwin { version }, WHY_KWIN_AUTO, None),
        CaptureBackend::Auto if portal => (CaptureChoice::Portal, WHY_PORTAL_FALLBACK, None),
        CaptureBackend::Auto => (CaptureChoice::InputIdleOnly, WHY_IDLE_ONLY, None),
    }
}

fn select_blank(probe: &Probe, configured: BlankMethod) -> BlankChoice {
    let dpms = probe.tools.kscreen_doctor && probe.wayland.kwin_dpms;
    let overlay = probe.wayland.layer_shell;
    let ddc = probe.tools.i2c;
    let available = match configured {
        BlankMethod::Dpms => dpms,
        BlankMethod::Overlay => overlay,
        BlankMethod::DdcStandby => ddc,
    };
    if available {
        return BlankChoice {
            method: configured,
            reason: blank_reason(configured),
            fell_back: false,
        };
    }
    if overlay && configured != BlankMethod::Overlay {
        return BlankChoice {
            method: BlankMethod::Overlay,
            reason: blank_fallback_reason(configured),
            fell_back: true,
        };
    }
    BlankChoice {
        method: configured,
        reason: blank_missing_reason(configured),
        fell_back: false,
    }
}

const fn blank_reason(method: BlankMethod) -> &'static str {
    match method {
        BlankMethod::Dpms => "kscreen-doctor and KWin DPMS",
        BlankMethod::Overlay => "layer-shell",
        BlankMethod::DdcStandby => "i2c",
    }
}

const fn blank_fallback_reason(configured: BlankMethod) -> &'static str {
    match configured {
        BlankMethod::Dpms => "dpms isn't available; using overlay",
        BlankMethod::DdcStandby => "ddc_standby isn't available; using overlay",
        BlankMethod::Overlay => "layer-shell",
    }
}

const fn blank_missing_reason(configured: BlankMethod) -> &'static str {
    match configured {
        BlankMethod::Dpms => "dpms isn't available, and layer-shell isn't either",
        BlankMethod::Overlay => "layer-shell isn't available",
        BlankMethod::DdcStandby => "i2c isn't available, and layer-shell isn't either",
    }
}

fn select_prompt(probe: &Probe, style: PromptStyle) -> PromptChoice {
    let notifications = probe.dbus.notifications;
    match style {
        PromptStyle::Dialog => PromptChoice {
            medium: PromptMedium::Dialog,
            reason: "configured",
        },
        PromptStyle::Notification if notifications => PromptChoice {
            medium: PromptMedium::Notification,
            reason: "configured",
        },
        PromptStyle::Auto if notifications => PromptChoice {
            medium: PromptMedium::Notification,
            reason: "auto",
        },
        PromptStyle::Notification | PromptStyle::Auto => PromptChoice {
            medium: PromptMedium::Dialog,
            reason: "notifications aren't available",
        },
    }
}

const WHY_KWIN_AUTO: &str = "KWin ScreenShot2 is present and authorized";
const WHY_KWIN_FORCED: &str = "capture.backend is kwin";
const WHY_KWIN_MISSING: &str = "KWin ScreenShot2 isn't available";
const WHY_KWIN_UNAUTHORIZED: &str = "KWin ScreenShot2 isn't authorized";
const WHY_PORTAL_FORCED: &str = "capture.backend is portal";
const WHY_PORTAL_MISSING: &str = "portal ScreenCast isn't available";
const WHY_PORTAL_FALLBACK: &str = "KWin ScreenShot2 isn't available; using portal ScreenCast";
const WHY_IDLE_ONLY: &str = "no capture backend; input idle only";
