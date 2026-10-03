//! Runs one batch of state-machine commands.

use std::sync::Arc;

use super::engine::Engine;
use super::inbox::Incoming;
use crate::config_watch::ReloadSignal;
use stillwatch_core::backend::{BackendError, ScreenCapture};
use stillwatch_core::command::Command;
use stillwatch_core::event::{CaptureFrame, Event};
use stillwatch_core::prompt::{PromptRequest, Reminder};
use stillwatch_core::time::TimerId;

impl<R: ReloadSignal> Engine<R> {
    pub(super) async fn apply(&mut self, commands: Vec<Command>) {
        let mut cancel_capture = false;
        for command in commands {
            if self.one(command, &mut cancel_capture).await {
                cancel_capture = false;
            }
        }
        if cancel_capture {
            self.stop_capture().await;
        }
    }

    /// Returns whether this command asked for a capture (so a cancel in the
    /// same batch doesn't drop it).
    async fn one(&mut self, command: Command, cancel_capture: &mut bool) -> bool {
        match command {
            Command::SetTimer { id, after } => {
                self.timers.schedule_after(id, self.clock.now(), after);
            }
            Command::CancelTimer(id) => {
                self.timers.cancel(id);
                if id == TimerId::Capture {
                    *cancel_capture = true;
                }
            }
            Command::RequestCapture {
                outputs,
                downscale_width,
            } => {
                self.start_capture(outputs, downscale_width).await;
                return true;
            }
            Command::ShowPrompt(request) => self.start_prompt(request),
            Command::DismissPrompt => self.dismiss_prompt().await,
            action @ (Command::Blank { .. } | Command::Lock) => self.spawn_action(action),
            action @ (Command::Unblank { .. } | Command::RunHook(_)) => {
                self.run_action(&action).await;
            }
            Command::Record(entry) => {
                if let Err(error) = self.shared.history.record(entry).await {
                    tracing::warn!(%error, "couldn't record history");
                }
            }
            Command::Notify(reminder) => self.remind(reminder),
            Command::StateChanged { to, .. } => emit_state(self.states.clone(), to).await,
        }
        false
    }

    async fn start_capture(&mut self, outputs: Vec<String>, width: u32) {
        self.stop_capture().await;
        let capture = super::shared::lock(&self.shared.capture).clone();
        let Some(capture) = capture else {
            self.capture_unavailable();
            return;
        };
        let portal = super::shared::lock(&self.shared.portal).clone();
        if let Some(portal) = portal {
            if let Err(error) = portal.set_away().await {
                tracing::warn!(%error, "couldn't start portal capture");
                let _ = self
                    .out
                    .send(Incoming::Event(Event::CaptureFailed { error }));
                return;
            }
            self.portal_away = true;
        }
        let generation = self.capture_gen;
        let out = self.out.clone();
        self.capture_task = Some(tokio::spawn(async move {
            let event = capture_outputs(capture, &outputs, width).await;
            let _ = out.send(Incoming::Capture(generation, event));
        }));
    }

    fn capture_unavailable(&mut self) {
        if !self.capture_warned {
            tracing::warn!("capture is unavailable; running on input idle only");
            self.capture_warned = true;
        }
        let _ = self.out.send(Incoming::Event(Event::CaptureFailed {
            error: BackendError::Unavailable("no capture backend".into()),
        }));
    }

    pub(super) async fn stop_capture(&mut self) {
        self.capture_gen = self.capture_gen.wrapping_add(1);
        if let Some(task) = self.capture_task.take() {
            task.abort();
        }
        if !self.portal_away {
            return;
        }
        self.portal_away = false;
        let portal = super::shared::lock(&self.shared.portal).clone();
        if let Some(portal) = portal
            && let Err(error) = portal.set_active().await
        {
            tracing::warn!(%error, "couldn't stop portal capture");
        }
    }

    fn start_prompt(&mut self, request: PromptRequest) {
        self.prompt_gen = self.prompt_gen.wrapping_add(1);
        let generation = self.prompt_gen;
        if let Some(task) = self.prompt_task.take() {
            task.abort();
        }
        let prompter = Arc::clone(&self.prompter);
        let out = self.out.clone();
        self.prompt_task = Some(tokio::spawn(async move {
            let event = match prompter.show(request).await {
                Ok(outcome) => Event::PromptAnswered(outcome),
                Err(error) => prompt_failure(error),
            };
            let _ = out.send(Incoming::Prompt(generation, event));
        }));
    }

    async fn dismiss_prompt(&mut self) {
        self.prompt_gen = self.prompt_gen.wrapping_add(1);
        if let Some(task) = self.prompt_task.take() {
            task.abort();
        }
        if let Err(error) = self.prompter.dismiss().await {
            tracing::warn!(%error, "couldn't close the prompt");
        }
    }

    /// Blank and lock run beside the loop so a dim wait can be cancelled.
    fn spawn_action(&mut self, command: Command) {
        if let Some(task) = self.action_task.take() {
            task.abort();
        }
        let runner = self.runner.clone();
        let out = self.out.clone();
        self.action_task = Some(tokio::spawn(async move {
            if let Some(event) = runner.execute(&command).await {
                let _ = out.send(Incoming::Event(event));
            }
        }));
    }

    async fn run_action(&mut self, command: &Command) {
        if let Some(event) = self.runner.execute(command).await {
            let _ = self.out.send(Incoming::Event(event));
        }
    }

    fn remind(&self, reminder: Reminder) {
        let prompter = Arc::clone(&self.prompter);
        tokio::spawn(async move {
            if let Err(error) = prompter.remind(reminder).await {
                tracing::warn!(%error, "couldn't show a reminder");
            }
        });
    }
}

async fn emit_state(
    signals: Option<crate::service::ServiceSignals>,
    to: stillwatch_core::state::State,
) {
    let Some(signals) = signals else {
        return;
    };
    if let Err(error) = signals.state_changed(to).await {
        tracing::warn!(%error, "couldn't emit StateChanged");
    }
}

fn prompt_failure(error: BackendError) -> Event {
    tracing::warn!(%error, "prompt failed");
    Event::PromptFailed { error }
}

async fn capture_outputs(capture: Arc<dyn ScreenCapture>, outputs: &[String], width: u32) -> Event {
    let targets = match resolve_outputs(capture.as_ref(), outputs).await {
        Ok(targets) => targets,
        Err(error) => return Event::CaptureFailed { error },
    };
    let mut frames = Vec::with_capacity(targets.len());
    for output in targets {
        match capture.capture_luma(&output, width).await {
            Ok(grid) => frames.push(CaptureFrame { output, grid }),
            Err(error) => return Event::CaptureFailed { error },
        }
    }
    Event::CaptureCompleted { frames }
}

async fn resolve_outputs(
    capture: &dyn ScreenCapture,
    outputs: &[String],
) -> Result<Vec<String>, BackendError> {
    if !outputs.is_empty() {
        return Ok(outputs.to_vec());
    }
    let listed = capture.outputs().await?;
    Ok(listed.into_iter().map(|output| output.name).collect())
}
