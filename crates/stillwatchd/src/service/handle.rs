//! What the D-Bus service needs from the rest of the daemon.

use std::time::Duration;

use futures_util::stream::BoxStream;
use jiff::Timestamp;
use stillwatch_core::backend::{BackendFuture, GamepadDevice};
use stillwatch_core::event::ControlCommand;
use stillwatch_core::history::HistoryEntry;
use stillwatch_core::panel::PanelRecord;
use stillwatch_core::prompt::PromptOutcome;
use stillwatch_core::state::{SnoozeError, StatusSnapshot};
use stillwatch_ipc::probe::ProbeSample;
use stillwatch_ipc::status::{PanelCareStatus, StatusPayload};

/// The daemon as seen by the D-Bus service.
///
/// The service validates arguments and translates calls into these methods;
/// everything else (the state machine, config, history, capture) stays behind
/// the handle. Object-safe and `Send + Sync` like the core backends, so the
/// service holds it as `Arc<dyn DaemonHandle>`.
pub trait DaemonHandle: Send + Sync {
    /// The current status.
    fn status(&self) -> BackendFuture<'_, DaemonStatus>;

    /// Checks a snooze length against the current `[prompt]` rules, usually
    /// with [`stillwatch_core::state::validate_snooze`].
    ///
    /// # Errors
    ///
    /// Returns the rule `duration` breaks.
    fn validate_snooze(&self, duration: Duration) -> Result<Duration, SnoozeError>;

    /// Feeds `Event::Control(command)` to the state machine. Snoozes have
    /// already passed [`DaemonHandle::validate_snooze`]; `Reload` is never
    /// sent here.
    fn control(&self, command: ControlCommand) -> BackendFuture<'_, ()>;

    /// Feeds `Event::PromptAnswered(outcome)` to the state machine. Snoozes
    /// have already passed [`DaemonHandle::validate_snooze`].
    fn answer_prompt(&self, outcome: PromptOutcome) -> BackendFuture<'_, ()>;

    /// Reloads the config now. The daemon's reload path also emits
    /// `ConfigChanged` (through `ServiceSignals`), as it does for the file
    /// watcher and SIGHUP; the service only returns the report to the caller.
    fn reload(&self) -> BackendFuture<'_, ReloadReport>;

    /// History entries with `at >= since`, oldest first.
    fn history(&self, since: Timestamp) -> BackendFuture<'_, Vec<HistoryEntry>>;

    /// Starts a calibration probe that samples every `interval`.
    ///
    /// The probe runs while the stream is alive: the service drops it when
    /// the last subscriber stops or disconnects, and the daemon stops
    /// capturing then. Capture failures are the daemon's to log; the stream
    /// just skips that sample.
    ///
    /// Called with the service's subscriber lock held, so it should only set
    /// the stream up; the work starts when the stream is polled.
    fn probe(&self, interval: Duration) -> BoxStream<'static, ProbeSample>;

    /// Connected output names.
    fn outputs(&self) -> BackendFuture<'_, Vec<String>>;

    /// Detected gamepads, as from `GamepadSource::devices`.
    fn gamepads(&self) -> Vec<GamepadDevice>;

    /// MPRIS player names, as from `MediaWatcher::players`.
    fn players(&self) -> BackendFuture<'_, Vec<String>>;
}

/// Everything `Status()` reports: the state machine's snapshot plus what only
/// the daemon knows.
#[derive(Debug, Clone, PartialEq)]
pub struct DaemonStatus {
    /// From `StateMachine::status`.
    pub snapshot: StatusSnapshot,
    /// The active capture backend (`kwin`, `portal`), or `None` in
    /// input-idle-only mode.
    pub capture_backend: Option<String>,
    /// Errors from the last failed reload; empty when the config is good.
    pub config_errors: Vec<String>,
    /// Panel care tracking, when enabled.
    pub panel_care: Option<PanelCareStatus>,
}

impl DaemonStatus {
    /// A status with nothing beyond the snapshot.
    #[must_use]
    pub const fn new(snapshot: StatusSnapshot) -> Self {
        Self {
            snapshot,
            capture_backend: None,
            config_errors: Vec::new(),
            panel_care: None,
        }
    }

    /// Fills panel care from the state machine's record. `None` leaves it off
    /// the status, which is what `panel_care.enabled = false` reports.
    #[must_use]
    pub fn with_panel(mut self, record: Option<PanelRecord>) -> Self {
        self.panel_care = record.map(PanelCareStatus::from);
        self
    }

    /// The wire payload.
    #[must_use]
    pub fn into_payload(self) -> StatusPayload {
        StatusPayload {
            capture_backend: self.capture_backend,
            config_errors: self.config_errors,
            panel_care: self.panel_care,
            ..StatusPayload::from_snapshot(&self.snapshot)
        }
    }
}

/// The result of a reload attempt, as `Reload()` and `ConfigChanged` carry it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReloadReport {
    /// Whether the new config was valid and applied.
    pub ok: bool,
    /// Why it wasn't, one message per problem.
    pub errors: Vec<String>,
}

impl ReloadReport {
    /// A successful reload.
    #[must_use]
    pub const fn applied() -> Self {
        Self {
            ok: true,
            errors: Vec::new(),
        }
    }

    /// A failed reload; the last good config stays in effect.
    #[must_use]
    pub const fn rejected(errors: Vec<String>) -> Self {
        Self { ok: false, errors }
    }
}

#[cfg(test)]
mod tests {
    use stillwatch_core::state::State;

    use super::*;

    fn snapshot() -> StatusSnapshot {
        StatusSnapshot {
            state: State::Monitoring,
            in_state: Duration::from_secs(7),
            snooze_remaining: None,
            idle: true,
            locked: false,
            media_playing: false,
            last_detection: None,
        }
    }

    #[test]
    fn payload_adds_daemon_fields_to_the_snapshot() {
        let panel = PanelCareStatus {
            screen_on_seconds: 60,
            last_standby: None,
            overlay_uses: 1,
        };
        let status = DaemonStatus {
            capture_backend: Some("kwin".into()),
            config_errors: vec!["stale.stale_percent: must be 1-100".into()],
            panel_care: Some(panel),
            ..DaemonStatus::new(snapshot())
        };
        assert_eq!(
            status.into_payload(),
            StatusPayload {
                capture_backend: Some("kwin".into()),
                config_errors: vec!["stale.stale_percent: must be 1-100".into()],
                panel_care: Some(panel),
                ..StatusPayload::from_snapshot(&snapshot())
            }
        );
    }

    #[test]
    fn panel_record_is_the_status_section() {
        let record = PanelRecord {
            screen_on_seconds: 3 * 3600,
            last_standby: Some(Timestamp::from_second(1_700_000_000).unwrap()),
            overlay_uses: 2,
        };
        let status = DaemonStatus::new(snapshot()).with_panel(Some(record));
        assert_eq!(
            status.panel_care,
            Some(PanelCareStatus {
                screen_on_seconds: 3 * 3600,
                last_standby: record.last_standby,
                overlay_uses: 2,
            })
        );
        assert!(
            DaemonStatus::new(snapshot())
                .with_panel(None)
                .panel_care
                .is_none()
        );
    }

    #[test]
    fn reports_say_whether_the_config_applied() {
        assert_eq!(
            ReloadReport::applied(),
            ReloadReport {
                ok: true,
                errors: Vec::new()
            }
        );
        let rejected = ReloadReport::rejected(vec!["bad".into()]);
        assert!(!rejected.ok);
        assert_eq!(rejected.errors, ["bad"]);
    }
}
