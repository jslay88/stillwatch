//! End-to-end wiring: mocks and a [`FakeClock`](stillwatch_core::time::FakeClock),
//! no compositor and no real displays.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use stillwatch_core::backend::{BackendError, BackendFuture, EventSink, GamepadSource, IdleSource};
use stillwatch_core::event::{ActivityEvent, Event};
use stillwatch_core::history::{HistoryKind, PromptAnswer};
use stillwatch_core::luma::{LumaGrid, OutputInfo};
use stillwatch_core::mocks::{
    BlankerCall, MemoryHistory, MockBlanker, MockCapture, MockMediaWatcher, MockPrompter,
    MockSessionMonitor, ScriptedDetector,
};
use stillwatch_core::prompt::PromptOutcome;
use stillwatch_core::state::State;
use stillwatch_core::time::{Clock, FakeClock};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio::time::timeout;

use super::engine::{Engine, NopSignal, Wiring};
use super::inbox::Incoming;
use super::parts::{ApplyConfig, Built, CaptureSession};
use super::shared::{Shared, lock};
use crate::config_watch::{ReloadTrigger, Reloader};
use crate::process::scripted::ScriptedRunner;
use crate::service::ReloadReport;
use crate::signals::{Signal, SignalSource};

const WAIT: Duration = Duration::from_secs(3);

const FAST: &str = "\
[stale]
check_interval_seconds = 1

[prompt]
countdown_seconds = 1

[session]
locked_blank_seconds = 1

[safety]
ceiling_enabled = false
";

/// An idle or gamepad source whose events are injected after the loop starts.
struct Pipe {
    rx: Mutex<Option<mpsc::UnboundedReceiver<Event>>>,
}

impl Pipe {
    fn pair() -> (mpsc::UnboundedSender<Event>, Arc<Self>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (
            tx,
            Arc::new(Self {
                rx: Mutex::new(Some(rx)),
            }),
        )
    }

    fn forward(&self, sink: Arc<dyn EventSink>) -> BackendFuture<'static, ()> {
        let rx = lock(&self.rx).take();
        Box::pin(async move {
            let Some(mut rx) = rx else {
                std::future::pending::<()>().await;
                return Ok(());
            };
            while let Some(event) = rx.recv().await {
                sink.send(event);
            }
            Ok(())
        })
    }
}

impl IdleSource for Pipe {
    fn watch(&self, _: Duration, sink: Arc<dyn EventSink>) -> BackendFuture<'_, ()> {
        self.forward(sink)
    }
}

impl GamepadSource for Pipe {
    fn watch(&self, sink: Arc<dyn EventSink>) -> BackendFuture<'_, ()> {
        self.forward(sink)
    }

    fn devices(&self) -> Vec<stillwatch_core::backend::GamepadDevice> {
        Vec::new()
    }
}

struct ChanSignals {
    rx: mpsc::UnboundedReceiver<Signal>,
}

impl SignalSource for ChanSignals {
    async fn recv(&mut self) -> Option<Signal> {
        self.rx.recv().await
    }
}

struct Rig {
    clock: FakeClock,
    out: mpsc::UnboundedSender<Incoming>,
    idle_tx: mpsc::UnboundedSender<Event>,
    pad_tx: mpsc::UnboundedSender<Event>,
    signals: mpsc::UnboundedSender<Signal>,
    blanker: Arc<MockBlanker>,
    capture: Option<Arc<MockCapture>>,
    history: Arc<MemoryHistory>,
    prompter: Arc<MockPrompter>,
    shared: Arc<Shared>,
    join: JoinHandle<()>,
    dir: tempfile::TempDir,
}

struct Parts {
    detector: ScriptedDetector,
    prompter: Arc<MockPrompter>,
    session: Arc<MockSessionMonitor>,
    capture: Option<Arc<MockCapture>>,
    stream: Option<Arc<dyn CaptureSession>>,
}

impl Rig {
    async fn start(parts: Parts) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, FAST).unwrap();
        let (reloader, loaded) = Reloader::load(path).unwrap();
        let clock = FakeClock::new();
        let (idle_tx, idle) = Pipe::pair();
        let (pad_tx, pads) = Pipe::pair();
        let blanker = Arc::new(MockBlanker::new());
        let history = Arc::new(MemoryHistory::new());
        let built = test_built(
            &reloader,
            &clock,
            idle,
            pads,
            blanker.clone(),
            history.clone(),
            parts,
        );
        let (out, inbox) = mpsc::unbounded_channel();
        let (activity_tx, _) = tokio::sync::watch::channel(
            stillwatch_core::activity::ActivitySettings::from(reloader.config()),
        );
        let shared = Arc::new(Shared::new(&built, out.clone()));
        let engine = Engine::new(
            built,
            reloader,
            loaded,
            Wiring {
                shared: Arc::clone(&shared),
                inbox,
                out: out.clone(),
                activity_tx,
                reload_signal: NopSignal,
                states: None,
                panel: crate::panel::PanelStore::new(dir.path().join("panel.json")),
            },
        );
        let (sig_tx, sig_rx) = mpsc::unbounded_channel();
        let join = tokio::spawn(async move {
            let mut signals = ChanSignals { rx: sig_rx };
            engine.run(&mut signals).await;
        });
        // Let boot finish (is_locked is immediate on the mock).
        tokio::task::yield_now().await;
        Self {
            clock,
            out,
            idle_tx,
            pad_tx,
            signals: sig_tx,
            blanker,
            capture: None,
            history,
            prompter: Arc::new(MockPrompter::new()),
            shared,
            join,
            dir,
        }
    }

    fn remember_capture(
        mut self,
        capture: Option<Arc<MockCapture>>,
        prompter: Arc<MockPrompter>,
    ) -> Self {
        self.capture = capture;
        self.prompter = prompter;
        self
    }

    async fn status(&self) -> stillwatch_core::state::StatusSnapshot {
        let (tx, rx) = oneshot::channel();
        self.out.send(Incoming::Status(tx)).unwrap();
        timeout(WAIT, rx)
            .await
            .expect("status")
            .expect("loop alive")
            .snapshot
    }

    async fn until_state(&self, want: State) {
        let deadline = tokio::time::Instant::now() + WAIT;
        loop {
            let status = self.status().await;
            if status.state == want {
                return;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "stuck in {:?}",
                status.state
            );
            tokio::task::yield_now().await;
        }
    }

    fn idle(&self) {
        self.idle_tx
            .send(Event::Activity(ActivityEvent::InputIdle))
            .unwrap();
    }

    fn gamepad(&self) {
        self.pad_tx
            .send(Event::Activity(ActivityEvent::GamepadActivity {
                device: "pad0".into(),
            }))
            .unwrap();
    }

    fn advance(&self, by: Duration) {
        self.clock.advance(by);
        self.out.send(Incoming::Tick).unwrap();
    }

    async fn stop(mut self) {
        self.signals.send(Signal::Terminate).unwrap();
        timeout(WAIT, &mut self.join)
            .await
            .expect("shutdown")
            .unwrap();
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.join.abort();
    }
}

fn test_built(
    reloader: &Reloader,
    clock: &FakeClock,
    idle: Arc<Pipe>,
    pads: Arc<Pipe>,
    blanker: Arc<MockBlanker>,
    history: Arc<MemoryHistory>,
    parts: Parts,
) -> Built {
    let config = reloader.config().clone();
    let clock: Arc<dyn Clock> = Arc::new(clock.clone());
    let apply_config: ApplyConfig = Arc::new(|_| Box::pin(async { Ok(()) }));
    let capture_backend = parts.capture.as_ref().map(|_| "kwin".to_owned());
    Built {
        config,
        idle,
        gamepad: Some(pads),
        gamepad_on: true,
        capture: parts
            .capture
            .clone()
            .map(|capture| capture as Arc<dyn stillwatch_core::backend::ScreenCapture>),
        portal: parts.stream.clone(),
        capture_backend,
        media: Arc::new(MockMediaWatcher::new()),
        prompter: parts.prompter.clone(),
        session: parts.session,
        dpms: blanker.clone(),
        overlay: blanker.clone(),
        ddc: blanker.clone(),
        dimmer: blanker,
        history,
        commands: Arc::new(ScriptedRunner::new()),
        clock,
        detector: Box::new(parts.detector),
        apply_config,
        watch_outputs: false,
    }
}

fn grid() -> LumaGrid {
    LumaGrid::filled(4, 2, 128).unwrap()
}

fn capture_ready() -> Arc<MockCapture> {
    let capture = Arc::new(MockCapture::new());
    capture.set_outputs(vec![OutputInfo::new("HDMI-A-1", 1920, 1080)]);
    capture.push_grid(grid());
    capture
}

async fn rig(parts: Parts) -> Rig {
    let capture = parts.capture.clone();
    let prompter = Arc::clone(&parts.prompter);
    Rig::start(parts).await.remember_capture(capture, prompter)
}

fn stale_parts(capture: Option<Arc<MockCapture>>, prompter: Arc<MockPrompter>) -> Parts {
    let detector = ScriptedDetector::new();
    detector.push_verdict(true);
    Parts {
        detector,
        prompter,
        session: Arc::new(MockSessionMonitor::new()),
        capture,
        stream: None,
    }
}

/// Records portal away/active transitions. `set_away` and `set_active` return
/// immediately, so the loop's order is what the test sees.
struct RecordingStream {
    log: Mutex<Vec<&'static str>>,
}

impl RecordingStream {
    fn new() -> Self {
        Self {
            log: Mutex::new(Vec::new()),
        }
    }

    fn log(&self) -> Vec<&'static str> {
        lock(&self.log).clone()
    }
}

impl CaptureSession for RecordingStream {
    fn set_away(&self) -> BackendFuture<'_, ()> {
        lock(&self.log).push("away");
        Box::pin(async { Ok(()) })
    }

    fn set_active(&self) -> BackendFuture<'_, ()> {
        lock(&self.log).push("active");
        Box::pin(async { Ok(()) })
    }
}

#[tokio::test]
async fn prompt_to_blank_and_no_capture_while_active() {
    let capture = capture_ready();
    let prompter = Arc::new(MockPrompter::new());
    let rig = rig(stale_parts(
        Some(Arc::clone(&capture)),
        Arc::clone(&prompter),
    ))
    .await;

    assert!(capture.requests().is_empty(), "no capture while Active");
    rig.idle();
    rig.until_state(State::Prompting).await;
    assert!(
        rig.blanker
            .calls()
            .iter()
            .all(|call| !matches!(call, BlankerCall::Blank(_))),
        "prompting doesn't blank"
    );

    rig.advance(Duration::from_secs(1));
    rig.until_state(State::Blanked).await;
    assert!(
        rig.blanker
            .calls()
            .iter()
            .any(|call| matches!(call, BlankerCall::Blank(_))),
        "countdown blanks"
    );
    rig.stop().await;
}

#[tokio::test]
async fn portal_stream_runs_only_while_a_capture_is_wanted() {
    let stream = Arc::new(RecordingStream::new());
    let mut parts = stale_parts(Some(capture_ready()), Arc::new(MockPrompter::new()));
    parts.stream = Some(Arc::clone(&stream) as Arc<dyn CaptureSession>);
    let rig = rig(parts).await;

    assert!(stream.log().is_empty(), "no stream while Active");
    rig.idle();
    rig.until_state(State::Prompting).await;
    assert_eq!(stream.log(), ["away", "active"]);
    rig.stop().await;
}

#[tokio::test]
async fn snooze_does_not_blank() {
    let prompter = Arc::new(MockPrompter::new());
    prompter.push_outcome(PromptOutcome::Snooze(Duration::from_mins(15)));
    let rig = rig(stale_parts(Some(capture_ready()), prompter)).await;

    rig.idle();
    rig.until_state(State::Snoozed).await;
    assert!(
        rig.blanker
            .calls()
            .iter()
            .all(|call| !matches!(call, BlankerCall::Blank(_))),
        "{:?}",
        rig.blanker.calls()
    );
    rig.stop().await;
}

#[tokio::test]
async fn gamepad_wakes_a_blanked_display() {
    let rig = rig(stale_parts(
        Some(capture_ready()),
        Arc::new(MockPrompter::new()),
    ))
    .await;
    rig.idle();
    rig.until_state(State::Prompting).await;
    rig.advance(Duration::from_secs(1));
    rig.until_state(State::Blanked).await;

    rig.gamepad();
    rig.until_state(State::Active).await;
    assert!(
        rig.blanker
            .calls()
            .iter()
            .any(|call| matches!(call, BlankerCall::Unblank(_))),
        "{:?}",
        rig.blanker.calls()
    );
    rig.stop().await;
}

#[tokio::test]
async fn a_locked_session_blanks_after_the_delay() {
    let session = Arc::new(MockSessionMonitor::new());
    session.set_locked(true);
    let mut parts = stale_parts(Some(capture_ready()), Arc::new(MockPrompter::new()));
    parts.session = session;
    let rig = rig(parts).await;

    rig.until_state(State::Locked).await;
    assert!(
        rig.blanker
            .calls()
            .iter()
            .all(|call| !matches!(call, BlankerCall::Blank(_))),
        "the lock delay hasn't elapsed"
    );
    rig.advance(Duration::from_secs(1));
    rig.until_state(State::Blanked).await;
    rig.stop().await;
}

#[tokio::test]
async fn missing_capture_stays_on_input_idle() {
    let rig = rig(stale_parts(None, Arc::new(MockPrompter::new()))).await;
    rig.idle();
    rig.until_state(State::Monitoring).await;
    tokio::task::yield_now().await;
    let status = rig.status().await;
    assert_eq!(status.state, State::Monitoring);
    assert!(lock(&rig.shared.capture_backend).is_none());
    assert!(
        rig.blanker
            .calls()
            .iter()
            .all(|call| !matches!(call, BlankerCall::Blank(_))),
        "a missing capture is not treated as a stale screen"
    );
    rig.stop().await;
}

#[tokio::test]
async fn shutdown_unblanks() {
    let rig = rig(stale_parts(
        Some(capture_ready()),
        Arc::new(MockPrompter::new()),
    ))
    .await;
    rig.idle();
    rig.until_state(State::Prompting).await;
    rig.advance(Duration::from_secs(1));
    rig.until_state(State::Blanked).await;

    let blanker = Arc::clone(&rig.blanker);
    rig.stop().await;
    assert!(
        blanker
            .calls()
            .iter()
            .any(|call| matches!(call, BlankerCall::Unblank(_))),
        "{:?}",
        blanker.calls()
    );
}

#[tokio::test]
async fn an_unavailable_prompt_is_a_failed_answer() {
    let prompter = Arc::new(MockPrompter::new());
    prompter.push_error(BackendError::Unavailable("no notification server".into()));
    let rig = rig(stale_parts(Some(capture_ready()), prompter)).await;
    rig.idle();
    rig.until_state(State::Prompting).await;

    let deadline = tokio::time::Instant::now() + WAIT;
    loop {
        let failed = rig.history.entries().iter().any(|entry| {
            entry.kind == HistoryKind::PromptAnswered && entry.answer == Some(PromptAnswer::Failed)
        });
        if failed {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "no failed answer recorded"
        );
        tokio::task::yield_now().await;
    }
    assert!(
        rig.blanker
            .calls()
            .iter()
            .all(|call| !matches!(call, BlankerCall::Blank(_))),
        "the countdown hasn't run"
    );
    rig.stop().await;
}

#[tokio::test]
async fn deleting_the_config_keeps_the_last_good_one() {
    let rig = rig(stale_parts(
        Some(capture_ready()),
        Arc::new(MockPrompter::new()),
    ))
    .await;
    let path = rig.dir.path().join("config.toml");
    std::fs::remove_file(&path).unwrap();

    let (reply, rx) = oneshot::channel();
    rig.out
        .send(Incoming::Reload(ReloadTrigger::Requested, Some(reply)))
        .unwrap();
    let report: ReloadReport = timeout(WAIT, rx).await.unwrap().unwrap();
    assert!(!report.ok, "{report:?}");
    assert_eq!(lock(&rig.shared.config).prompt.countdown_seconds, 1);
    rig.stop().await;
}
