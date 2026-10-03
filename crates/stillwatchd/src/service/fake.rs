//! A scriptable [`DaemonHandle`] for tests of the service and its clients.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::StreamExt as _;
use futures_util::stream::{self, BoxStream};
use jiff::Timestamp;
use stillwatch_core::backend::{BackendError, BackendFuture, GamepadDevice};
use stillwatch_core::config::PromptConfig;
use stillwatch_core::event::ControlCommand;
use stillwatch_core::history::HistoryEntry;
use stillwatch_core::prompt::PromptOutcome;
use stillwatch_core::state::{SnoozeError, State, StatusSnapshot, validate_snooze};
use stillwatch_core::stats::{Threshold, ThresholdReason};
use stillwatch_ipc::probe::ProbeSample;
use tokio::sync::watch;

use super::handle::{DaemonHandle, DaemonStatus, ReloadReport};
use super::lock;

/// What a [`FakeHandle`] answers with, and what it was asked.
#[derive(Debug, Clone)]
pub struct FakeState {
    /// Returned by `status`.
    pub status: DaemonStatus,
    /// The rules `validate_snooze` applies.
    pub prompt: PromptConfig,
    /// Returned by `reload`.
    pub reload: ReloadReport,
    /// Filtered by `since` and returned by `history`.
    pub history: Vec<HistoryEntry>,
    /// Returned by `outputs`.
    pub outputs: Vec<String>,
    /// Returned by `gamepads`.
    pub gamepads: Vec<GamepadDevice>,
    /// Returned by `players`.
    pub players: Vec<String>,
    /// Yielded by every probe stream, once right away and then every
    /// interval.
    pub sample: ProbeSample,
    /// When set, every fallible call fails with it.
    pub failure: Option<BackendError>,
    /// Every `control` command, in order.
    pub controls: Vec<ControlCommand>,
    /// Every `answer_prompt` outcome, in order.
    pub answers: Vec<PromptOutcome>,
    /// How many times `reload` ran.
    pub reloads: usize,
    /// Every `since` passed to `history`.
    pub history_since: Vec<Timestamp>,
    /// Every interval a probe was started with.
    pub probe_intervals: Vec<Duration>,
}

impl Default for FakeState {
    fn default() -> Self {
        Self {
            status: DaemonStatus::new(StatusSnapshot {
                state: State::Active,
                in_state: Duration::ZERO,
                snooze_remaining: None,
                idle: false,
                locked: false,
                media_playing: false,
                last_detection: None,
            }),
            prompt: PromptConfig::default(),
            reload: ReloadReport::applied(),
            history: Vec::new(),
            outputs: Vec::new(),
            gamepads: Vec::new(),
            players: Vec::new(),
            sample: ProbeSample {
                at: Timestamp::UNIX_EPOCH,
                threshold: Threshold::new(70, ThresholdReason::Normal),
                stale: false,
                outputs: Vec::new(),
            },
            failure: None,
            controls: Vec::new(),
            answers: Vec::new(),
            reloads: 0,
            history_since: Vec::new(),
            probe_intervals: Vec::new(),
        }
    }
}

/// A [`DaemonHandle`] backed by a [`FakeState`] and a count of live probe
/// streams.
#[derive(Debug, Default)]
pub struct FakeHandle {
    state: Mutex<FakeState>,
    probes: Arc<watch::Sender<usize>>,
}

impl FakeHandle {
    /// A handle answering with [`FakeState::default`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Changes what the handle answers with.
    pub fn update(&self, change: impl FnOnce(&mut FakeState)) {
        change(&mut lock(&self.state));
    }

    /// A copy of the current state, including what was asked so far.
    #[must_use]
    pub fn state(&self) -> FakeState {
        lock(&self.state).clone()
    }

    /// How many probe streams are alive right now.
    #[must_use]
    pub fn live_probes(&self) -> usize {
        *self.probes.borrow()
    }

    /// Waits until exactly `count` probe streams are alive.
    pub async fn wait_for_probes(&self, count: usize) {
        let mut probes = self.probes.subscribe();
        let _ = probes.wait_for(|live| *live == count).await;
    }

    fn answer<T: Send + 'static>(
        &self,
        reply: impl FnOnce(&mut FakeState) -> T,
    ) -> BackendFuture<'_, T> {
        let mut state = lock(&self.state);
        let result = state
            .failure
            .clone()
            .map_or_else(|| Ok(reply(&mut state)), Err);
        Box::pin(std::future::ready(result))
    }
}

impl DaemonHandle for FakeHandle {
    fn status(&self) -> BackendFuture<'_, DaemonStatus> {
        self.answer(|state| state.status.clone())
    }

    fn validate_snooze(&self, duration: Duration) -> Result<Duration, SnoozeError> {
        validate_snooze(&lock(&self.state).prompt, duration)
    }

    fn control(&self, command: ControlCommand) -> BackendFuture<'_, ()> {
        self.answer(|state| state.controls.push(command))
    }

    fn answer_prompt(&self, outcome: PromptOutcome) -> BackendFuture<'_, ()> {
        self.answer(|state| state.answers.push(outcome))
    }

    fn reload(&self) -> BackendFuture<'_, ReloadReport> {
        self.answer(|state| {
            state.reloads += 1;
            state.reload.clone()
        })
    }

    fn history(&self, since: Timestamp) -> BackendFuture<'_, Vec<HistoryEntry>> {
        self.answer(|state| {
            state.history_since.push(since);
            state
                .history
                .iter()
                .filter(|entry| entry.at >= since)
                .cloned()
                .collect()
        })
    }

    fn probe(&self, interval: Duration) -> BoxStream<'static, ProbeSample> {
        let sample = {
            let mut state = lock(&self.state);
            state.probe_intervals.push(interval);
            state.sample.clone()
        };
        let live = LiveProbe::new(Arc::clone(&self.probes));
        stream::unfold((live, true), move |(live, first)| {
            let sample = sample.clone();
            async move {
                if !first {
                    tokio::time::sleep(interval).await;
                }
                Some((sample, (live, false)))
            }
        })
        .boxed()
    }

    fn outputs(&self) -> BackendFuture<'_, Vec<String>> {
        self.answer(|state| state.outputs.clone())
    }

    fn gamepads(&self) -> Vec<GamepadDevice> {
        lock(&self.state).gamepads.clone()
    }

    fn players(&self) -> BackendFuture<'_, Vec<String>> {
        self.answer(|state| state.players.clone())
    }
}

/// Counts a probe stream as live until the stream is dropped.
struct LiveProbe(Arc<watch::Sender<usize>>);

impl LiveProbe {
    fn new(probes: Arc<watch::Sender<usize>>) -> Self {
        probes.send_modify(|live| *live += 1);
        Self(probes)
    }
}

impl Drop for LiveProbe {
    fn drop(&mut self) {
        self.0.send_modify(|live| *live = live.saturating_sub(1));
    }
}
