//! Apply a platform selection: capture, blank fallback, status, history.

use stillwatch_core::command::Command;
use stillwatch_core::config::Config;
use stillwatch_core::history::HistoryEntry;

use super::engine::Engine;
use super::parts::{self, OpenedCapture};
use super::shared::lock;
use crate::config_watch::ReloadSignal;
use crate::platform::{self, Probe, Selection};

/// What startup probed and opened. The history ring is recorded by the caller,
/// after it exists.
pub(super) struct Startup {
    pub probe: Probe,
    pub selection: Selection,
    pub opened: OpenedCapture,
}

/// Probes, selects, and opens capture. Fails when idle or a forced capture
/// backend isn't available.
///
/// # Errors
///
/// The probe can't read the compositor, or [`Selection::startup_failure`]
/// is set, or a forced backend doesn't connect.
pub(super) async fn startup(config: &Config) -> anyhow::Result<Startup> {
    let probe = platform::probe_session().await?;
    let selection = platform::select(&probe, config);
    if let Some(message) = selection.startup_failure(&platform::current_exe()) {
        anyhow::bail!("{message}");
    }
    selection.log_selected();
    let opened = parts::open_choice(config, &selection).await?;
    Ok(Startup {
        probe,
        selection,
        opened,
    })
}

impl<R: ReloadSignal> Engine<R> {
    /// Probes again and reopens capture when `force` is set or the choice changed.
    pub(super) async fn refresh_capture(&mut self, config: &Config, force: bool) {
        let probe = match platform::probe_session().await {
            Ok(probe) => probe,
            Err(error) => {
                tracing::error!(%error, "platform probe failed");
                return;
            }
        };
        let selection = platform::select(&probe, config);
        if let Some(message) = selection.startup_failure(&platform::current_exe()) {
            tracing::error!("{message}");
        }
        let next = selection.capture_backend_name();
        let current = lock(&self.shared.capture_backend).clone();
        if force || next != current {
            self.stop_capture().await;
            let opened = match parts::open_choice(config, &selection).await {
                Ok(opened) => opened,
                Err(error) => {
                    tracing::error!(%error, "couldn't open the selected capture backend");
                    OpenedCapture::none()
                }
            };
            self.capture_warned = opened.capture.is_none();
            *lock(&self.shared.capture) = opened.capture;
            *lock(&self.shared.portal) = opened.portal;
            *lock(&self.shared.capture_backend) = opened.name;
        }
        self.probe = Some(probe);
        self.install_selection(&selection).await;
    }

    /// Re-runs selection against the last probe. Tests leave the probe empty.
    pub(super) async fn reselect(&mut self, config: &Config) {
        let Some(probe) = self.probe.clone() else {
            return;
        };
        let selection = platform::select(&probe, config);
        self.install_selection(&selection).await;
    }

    /// The compositor or a watched bus name changed.
    pub(super) async fn on_reprobe(&mut self) {
        let config = lock(&self.shared.config).clone();
        self.refresh_capture(&config, false).await;
    }

    pub(super) fn rewrite_blank(&self, command: Command) -> Command {
        match command {
            Command::Blank { outputs, method } => Command::Blank {
                outputs,
                method: self.blank_override.unwrap_or(method),
            },
            other => other,
        }
    }

    pub(super) fn rewrite_record(&self, mut entry: HistoryEntry) -> HistoryEntry {
        if let Some(method) = entry.blank_method
            && let Some(override_method) = self.blank_override
            && method != override_method
        {
            entry.blank_method = Some(override_method);
        }
        entry
    }

    async fn install_selection(&mut self, selection: &Selection) {
        self.blank_override = selection.blank_override();
        *lock(&self.shared.backends) = Some(selection.report());
        let names = selection.history_names();
        if names == self.selected_names {
            return;
        }
        selection.log_selected();
        let entry = selection.history_entry(self.clock.wall_now());
        if let Err(error) = self.shared.history.record(entry).await {
            tracing::warn!(%error, "couldn't record backend selection");
        }
        self.selected_names = names;
    }
}
