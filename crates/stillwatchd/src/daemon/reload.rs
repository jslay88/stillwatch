//! Config reload and shutdown. The loop is the only caller.

use std::sync::Arc;

use stillwatch_core::activity::ActivitySettings;
use stillwatch_core::command::Command;
use stillwatch_core::config::Config;

use super::engine::Engine;
use super::shared::lock;
use super::spawn::spawn_activity;
use crate::config_watch::{ReloadOutcome, ReloadSignal, ReloadTrigger, reload_and_report};
use crate::service::ReloadReport;

impl<R: ReloadSignal> Engine<R> {
    pub(super) async fn reload(&mut self, trigger: ReloadTrigger) -> ReloadReport {
        if trigger == ReloadTrigger::Hangup {
            tracing::info!("SIGHUP, reloading the config");
        }
        let outcome = reload_and_report(&mut self.reloader, trigger, &self.reload_signal).await;
        let commands =
            outcome.update_machine(&mut self.machine, self.clock.now(), self.clock.wall_now());
        *lock(&self.shared.errors) = self.reloader.errors().to_vec();
        if let ReloadOutcome::Applied(applied) = &outcome {
            let config = applied.loaded.config.clone();
            let capture_changed = applied.changes.contains("capture.backend");
            self.adopt(config, capture_changed).await;
        }
        self.apply(commands).await;
        outcome.report().unwrap_or_else(ReloadReport::applied)
    }

    async fn adopt(&mut self, config: Config, capture_changed: bool) {
        self.runner.apply_config(&config);
        let _ = self.activity_tx.send(ActivitySettings::from(&config));
        *lock(&self.shared.prompt) = config.prompt.clone();
        *lock(&self.shared.config) = config.clone();
        if capture_changed {
            let (capture, name) = super::parts::open_capture(&config).await;
            self.capture_warned = capture.is_none();
            *lock(&self.shared.capture) = capture;
            *lock(&self.shared.capture_backend) = name;
            self.publish_outputs().await;
        }
        if self.gamepad_on != config.activity.gamepad {
            self.gamepad_on = config.activity.gamepad;
            self.respawn_activity();
        }
        if let Err(error) = (self.apply_config)(config).await {
            tracing::warn!(%error, "reload side effects failed");
        }
    }

    fn respawn_activity(&mut self) {
        if let Some(job) = self.activity_job.take() {
            job.abort();
        }
        self.activity_job = Some(spawn_activity(
            Arc::clone(&self.idle),
            self.watched_gamepad(),
            self.activity_tx.subscribe(),
            Arc::clone(&self.clock),
            self.out.clone(),
        ));
    }

    /// Aborts in-flight blank and prompt work, then wakes anything we blanked.
    /// History writes are awaited as they happen. Panel care is flushed here.
    pub(super) async fn shutdown(&mut self) {
        self.prompt_gen = self.prompt_gen.wrapping_add(1);
        if let Some(task) = self.prompt_task.take() {
            task.abort();
        }
        if let Some(task) = self.action_task.take() {
            task.abort();
        }
        self.stop_capture();
        if let Some(task) = self.probe_task.take() {
            task.abort();
        }
        if let Some(task) = self.activity_job.take() {
            task.abort();
        }
        for job in self.jobs.drain(..) {
            job.abort();
        }
        if self.machine.displays_blanked() {
            let _ = self
                .runner
                .execute(&Command::Unblank {
                    outputs: Vec::new(),
                })
                .await;
        }
        if let Err(error) = self.prompter.dismiss().await {
            tracing::warn!(%error, "couldn't close the prompt");
        }
        if let Err(error) = self.panel.flush() {
            tracing::warn!(%error, "couldn't write panel care state");
        }
    }

    pub(super) fn persist_panel(&mut self) {
        let now = self.clock.now();
        if let Some(record) = self.machine.panel_record(now) {
            self.panel.update(record, now);
        }
        if let Err(error) = self.panel.flush_if_due(now) {
            tracing::warn!(%error, "couldn't write panel care state");
        }
    }
}
