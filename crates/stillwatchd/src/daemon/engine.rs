//! The select loop: backends and timers in, commands out.

#[cfg(test)]
use std::future::Future;
use std::sync::Arc;
use std::time::Instant;

use stillwatch_core::activity::ActivitySettings;
use stillwatch_core::backend::{
    BackendError, Blanker, GamepadSource, IdleSource, MediaWatcher, Prompter, SessionMonitor,
};
use stillwatch_core::command::BlankMethod;
use stillwatch_core::config::LoadOutcome;
use stillwatch_core::event::{Event, SessionEvent};
use stillwatch_core::state::StateMachine;
use stillwatch_core::time::{Clock, TimerQueue};
use stillwatch_ipc::probe::ProbeSample;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use super::inbox::Incoming;
use super::parts::{ApplyConfig, Built};
use super::shared::{Shared, lock};
use super::spawn::{spawn_activity, spawn_config_watcher, spawn_hotplug, spawn_sources};
use crate::action::ActionRunner;
use crate::config_watch::{ReloadSignal, ReloadTrigger, Reloader};
use crate::service::{DaemonStatus, ServiceSignals};
#[cfg(test)]
use crate::service::{ReloadReport, ServiceError};
use crate::signals::{Signal, SignalSource};

/// Channels and the reload reporter the loop is built around.
pub(super) struct Wiring<R> {
    pub shared: Arc<Shared>,
    pub inbox: mpsc::UnboundedReceiver<Incoming>,
    pub out: mpsc::UnboundedSender<Incoming>,
    pub activity_tx: watch::Sender<ActivitySettings>,
    pub reload_signal: R,
    pub states: Option<ServiceSignals>,
    pub panel: crate::panel::PanelStore,
}

/// Whether the loop watches `wl_output`.
enum Hotplug {
    Off,
    On,
}

/// Owns the state machine and every task the loop spawned.
pub(super) struct Engine<R> {
    pub(super) machine: StateMachine,
    pub(super) clock: Arc<dyn Clock>,
    pub(super) timers: TimerQueue,
    pub(super) runner: ActionRunner,
    pub(super) shared: Arc<Shared>,
    pub(super) inbox: mpsc::UnboundedReceiver<Incoming>,
    pub(super) out: mpsc::UnboundedSender<Incoming>,
    pub(super) activity_tx: watch::Sender<ActivitySettings>,
    pub(super) reload_signal: R,
    pub(super) states: Option<ServiceSignals>,
    pub(super) apply_config: ApplyConfig,
    pub(super) reloader: Reloader,
    pub(super) prompter: Arc<dyn Prompter>,
    pub(super) capture_gen: u64,
    pub(super) capture_task: Option<JoinHandle<()>>,
    pub(super) capture_warned: bool,
    /// A portal stream was started for the current capture. Shutdown and the
    /// next capture stop it before doing anything else.
    pub(super) portal_away: bool,
    pub(super) prompt_gen: u64,
    pub(super) prompt_task: Option<JoinHandle<()>>,
    pub(super) action_task: Option<JoinHandle<()>>,
    pub(super) panel: crate::panel::PanelStore,
    loaded: Option<LoadOutcome>,
    pub(super) idle: Arc<dyn IdleSource>,
    gamepad: Option<Arc<dyn GamepadSource>>,
    pub(super) gamepad_on: bool,
    media: Arc<dyn MediaWatcher>,
    session: Arc<dyn SessionMonitor>,
    dpms: Arc<dyn Blanker>,
    overlay: Arc<dyn Blanker>,
    ddc: Arc<dyn Blanker>,
    hotplug: Hotplug,
    pub(super) jobs: Vec<JoinHandle<()>>,
    pub(super) activity_job: Option<JoinHandle<()>>,
    pub(super) probe_task: Option<JoinHandle<()>>,
    /// Blank method that replaces the configured one.
    pub(super) blank_override: Option<BlankMethod>,
    /// Last platform probe. `None` in tests, so a reload doesn't touch the session.
    pub(super) probe: Option<crate::platform::Probe>,
    /// Backend names already logged and recorded.
    pub(super) selected_names: String,
}

impl<R: ReloadSignal> Engine<R> {
    pub(super) fn new(
        built: Built,
        reloader: Reloader,
        loaded: LoadOutcome,
        wiring: Wiring<R>,
    ) -> Self {
        let now = built.clock.now();
        let runner = built.runner();
        let (machine, _) = StateMachine::new(&built.config, built.detector, now);
        Self {
            machine,
            clock: Arc::clone(&built.clock),
            timers: TimerQueue::new(),
            runner,
            shared: wiring.shared,
            inbox: wiring.inbox,
            out: wiring.out,
            activity_tx: wiring.activity_tx,
            reload_signal: wiring.reload_signal,
            states: wiring.states,
            panel: wiring.panel,
            apply_config: built.apply_config,
            reloader,
            prompter: built.prompter,
            capture_gen: 0,
            capture_task: None,
            capture_warned: built.capture.is_none(),
            portal_away: false,
            prompt_gen: 0,
            prompt_task: None,
            action_task: None,
            loaded: Some(loaded),
            idle: built.idle,
            gamepad: built.gamepad,
            gamepad_on: built.gamepad_on,
            media: built.media,
            session: built.session,
            dpms: built.dpms,
            overlay: built.overlay,
            ddc: built.ddc,
            hotplug: if built.watch_outputs {
                Hotplug::On
            } else {
                Hotplug::Off
            },
            jobs: Vec::new(),
            activity_job: None,
            probe_task: None,
            blank_override: built.blank_override,
            probe: built.probe,
            selected_names: built.selected_names,
        }
    }

    /// Boots, then selects until a stop signal. Shutdown unblanks first.
    pub(super) async fn run(mut self, signals: &mut impl SignalSource) {
        if !self.boot(signals).await {
            return;
        }
        self.spawn_jobs();
        loop {
            self.persist_panel();
            if self.fire_due().await {
                continue;
            }
            let deadline = self.timers.next_deadline();
            let clock = Arc::clone(&self.clock);
            tokio::select! {
                biased;
                Some(message) = self.inbox.recv() => self.dispatch(message).await,
                signal = signals.recv() => {
                    if self.on_signal(signal).await {
                        break;
                    }
                }
                () = sleep_deadline(deadline, clock.as_ref()) => {}
            }
        }
    }

    async fn boot(&mut self, signals: &mut impl SignalSource) -> bool {
        let saved = self.panel.load();
        self.machine.restore_panel(saved);
        self.note_migration().await;
        self.publish_outputs().await;
        self.locked_now(signals).await
    }

    async fn note_migration(&mut self) {
        let Some(loaded) = self.loaded.take() else {
            return;
        };
        let commands =
            self.machine
                .config_migrated(self.clock.now(), self.clock.wall_now(), &loaded);
        self.apply(commands).await;
    }

    pub(super) async fn publish_outputs(&mut self) {
        let capture = lock(&self.shared.capture).clone();
        let Some(capture) = capture else {
            return;
        };
        match capture.outputs().await {
            Ok(outputs) => self.on_event(&Event::OutputsChanged(outputs)).await,
            Err(error) => tracing::warn!(%error, "couldn't list outputs"),
        }
    }

    /// `is_locked` once, before the session watch (which doesn't emit the
    /// current state). A stop signal wins over a slow logind call.
    async fn locked_now(&mut self, signals: &mut impl SignalSource) -> bool {
        loop {
            tokio::select! {
                biased;
                signal = signals.recv() => {
                    if self.on_signal(signal).await {
                        return false;
                    }
                }
                locked = self.session.is_locked() => {
                    self.note_lock(locked).await;
                    return true;
                }
            }
        }
    }

    async fn note_lock(&mut self, locked: Result<bool, BackendError>) {
        match locked {
            Ok(true) => self.on_event(&Event::Session(SessionEvent::Locked)).await,
            Ok(false) => {}
            Err(error) => tracing::warn!(%error, "couldn't read the lock state"),
        }
    }

    fn spawn_jobs(&mut self) {
        let gamepad = self.watched_gamepad();
        self.activity_job = Some(spawn_activity(
            Arc::clone(&self.idle),
            gamepad,
            self.activity_tx.subscribe(),
            Arc::clone(&self.clock),
            self.out.clone(),
        ));
        self.jobs = spawn_sources(
            Arc::clone(&self.session),
            Arc::clone(&self.media),
            Arc::clone(&self.dpms),
            Arc::clone(&self.overlay),
            Arc::clone(&self.ddc),
            Arc::clone(&self.clock),
            self.out.clone(),
        );
        if matches!(self.hotplug, Hotplug::On) {
            self.jobs
                .push(spawn_hotplug(Arc::clone(&self.clock), self.out.clone()));
            self.jobs
                .push(super::spawn::spawn_platform_watch(self.out.clone()));
        }
        if let Some(job) = spawn_config_watcher(self.reloader.path(), self.out.clone()) {
            self.jobs.push(job);
        }
    }

    pub(super) fn watched_gamepad(&self) -> Option<Arc<dyn GamepadSource>> {
        self.gamepad_on.then(|| self.gamepad.clone()).flatten()
    }

    async fn fire_due(&mut self) -> bool {
        let due = self.timers.pop_due(self.clock.now());
        if due.is_empty() {
            return false;
        }
        for id in due {
            self.on_event(&Event::Timer(id)).await;
        }
        true
    }

    async fn dispatch(&mut self, message: Incoming) {
        match message {
            Incoming::Event(event) => self.on_event(&event).await,
            Incoming::Capture(generation, event) => {
                self.capture_result(generation, event).await;
            }
            Incoming::Prompt(generation, event) => self.prompt_result(generation, event).await,
            Incoming::Reload(trigger, reply) => {
                let report = self.reload(trigger).await;
                if let Some(reply) = reply {
                    let _ = reply.send(report);
                }
            }
            Incoming::Control(command, reply) => {
                self.on_event(&Event::Control(command)).await;
                let _ = reply.send(());
            }
            Incoming::Answer(outcome, reply) => {
                self.on_event(&Event::PromptAnswered(outcome)).await;
                let _ = reply.send(());
            }
            Incoming::Status(reply) => {
                let _ = reply.send(self.status_now());
            }
            Incoming::Probe(interval, tx) => self.spawn_probe(interval, tx),
            Incoming::Tick => {}
            Incoming::Reprobe => self.on_reprobe().await,
        }
    }

    pub(super) async fn on_event(&mut self, event: &Event) {
        if let Event::OutputsChanged(outputs) = event {
            let names = outputs.iter().map(|output| output.name.clone()).collect();
            self.runner.set_connected(names);
        }
        if let Event::Media { playing } = event {
            lock(&self.shared.playing).clone_from(playing);
        }
        let commands = self
            .machine
            .handle(self.clock.now(), self.clock.wall_now(), event);
        self.apply(commands).await;
    }

    async fn capture_result(&mut self, generation: u64, event: Event) {
        if generation != self.capture_gen {
            return;
        }
        if let Event::CaptureFailed { error } = &event {
            tracing::warn!(%error, "capture failed");
            if matches!(error, BackendError::PermissionDenied(_)) {
                *lock(&self.shared.capture) = None;
                *lock(&self.shared.capture_backend) = None;
                self.capture_warned = true;
            }
        }
        self.on_event(&event).await;
    }

    async fn prompt_result(&mut self, generation: u64, event: Event) {
        if generation == self.prompt_gen {
            self.on_event(&event).await;
        }
    }

    fn spawn_probe(&mut self, interval: std::time::Duration, tx: mpsc::Sender<ProbeSample>) {
        if let Some(task) = self.probe_task.take() {
            task.abort();
        }
        let shared = Arc::clone(&self.shared);
        let clock = Arc::clone(&self.clock);
        self.probe_task = Some(tokio::spawn(super::probe_loop::run(
            shared, clock, interval, tx,
        )));
    }

    /// `true` when the process should exit.
    async fn on_signal(&mut self, signal: Option<Signal>) -> bool {
        match signal {
            Some(Signal::Hangup) => {
                self.reload(ReloadTrigger::Hangup).await;
                false
            }
            other => {
                let signal = other.unwrap_or(Signal::Terminate);
                tracing::info!(?signal, "stillwatchd stopping");
                self.shutdown().await;
                true
            }
        }
    }

    pub(super) fn status_now(&self) -> DaemonStatus {
        let now = self.clock.now();
        DaemonStatus {
            snapshot: self.machine.status(now),
            capture_backend: lock(&self.shared.capture_backend).clone(),
            backends: lock(&self.shared.backends).clone(),
            config_errors: lock(&self.shared.errors).clone(),
            panel_care: None,
        }
        .with_panel(self.machine.panel_record(now))
    }
}

async fn sleep_deadline(deadline: Option<Instant>, clock: &dyn Clock) {
    let Some(deadline) = deadline else {
        std::future::pending::<()>().await;
        return;
    };
    let left = deadline.saturating_duration_since(clock.now());
    if !left.is_zero() {
        tokio::time::sleep(left).await;
    }
}

/// Reload reporter for tests, where nothing is listening for `ConfigChanged`.
#[cfg(test)]
#[derive(Debug, Default)]
pub(super) struct NopSignal;

#[cfg(test)]
impl ReloadSignal for NopSignal {
    fn config_changed(
        &self,
        _: &ReloadReport,
    ) -> impl Future<Output = Result<(), ServiceError>> + Send {
        std::future::ready(Ok(()))
    }
}
