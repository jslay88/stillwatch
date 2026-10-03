use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use jiff::Timestamp;

use super::State;
use super::detector::StaleDetector;
use super::snooze::{prompt_presets, validate_snooze};
use crate::command::{BlankMethod, Command, HookKind};
use crate::config::{ActionOutputs, Config};
use crate::event::CaptureFrame;
use crate::history::{DecisionContext, HistoryEntry, HistoryKind};
use crate::luma::OutputInfo;
use crate::panel::PanelTracker;
use crate::prompt::{PromptRequest, StaleOutput};
use crate::stats::DetectionStats;
use crate::time::TimerId;

mod hotplug;

/// Where a handler wants to go, plus the details its history entry carries.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Transition {
    pub(super) to: State,
    pub(super) detection: Option<DetectionStats>,
    pub(super) snooze: Option<Duration>,
    pub(super) blank_method: Option<BlankMethod>,
    pub(super) reblank_attempt: Option<u32>,
}

impl Transition {
    pub(super) const fn to(to: State) -> Self {
        Self {
            to,
            detection: None,
            snooze: None,
            blank_method: None,
            reblank_attempt: None,
        }
    }

    pub(super) fn with_detection(mut self, detection: DetectionStats) -> Self {
        self.detection = Some(detection);
        self
    }

    pub(super) const fn with_snooze(mut self, snooze: Duration) -> Self {
        self.snooze = Some(snooze);
        self
    }

    pub(super) const fn with_blank_method(mut self, method: BlankMethod) -> Self {
        self.blank_method = Some(method);
        self
    }

    pub(super) const fn with_reblank_attempt(mut self, attempt: u32) -> Self {
        self.reblank_attempt = Some(attempt);
        self
    }

    pub(super) fn history_entry(&self, at: Timestamp, from: State) -> HistoryEntry {
        let mut entry = HistoryEntry::transition(at, from, self.to);
        entry.detection.clone_from(&self.detection);
        entry.snooze_seconds = self.snooze.map(|snooze| snooze.as_secs());
        entry.blank_method = self.blank_method;
        entry.reblank_attempt = self.reblank_attempt;
        entry
    }
}

/// One step of the configured action, run in order while Acting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ActionStep {
    Lock,
    Blank(BlankMethod),
}

/// Whether the compositor idle watch is up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Compositor {
    /// Up, or not yet reported down. Input idle may blank.
    Up,
    /// The watch returned an error. Activity is unknown.
    Down,
}

/// Everything the handlers share: config, detector, observed facts, and the
/// commands produced by the current step.
pub(super) struct Ctx {
    pub(super) config: Config,
    pub(super) detector: Box<dyn StaleDetector>,
    pub(super) now: Instant,
    pub(super) wall: Timestamp,
    out: Vec<Command>,
    /// Aggregated input idle: compositor idle and no recent gamepad input.
    pub(super) idle: bool,
    /// The compositor idle watch. Starts [`Compositor::Up`] so a machine that
    /// has not heard from the watch still blanks on input idle.
    compositor: Compositor,
    /// How many times this process has lost the compositor idle watch.
    reconnects: u32,
    /// `None` until the first `OutputsChanged`. `Some` is the connected list,
    /// which may be empty.
    outputs: Option<Vec<OutputInfo>>,
    /// Generations already counted as a wake-without-input this blank episode.
    hotplug_wakes: HashMap<String, u64>,
    /// Output generation at the moment Blanked was entered.
    blank_generation: HashMap<String, u64>,
    pub(super) locked: bool,
    pub(super) playing: Vec<String>,
    pub(super) last_gamepad: Option<Instant>,
    pub(super) last_detection: Option<DetectionStats>,
    pub(super) snooze_until: Option<Instant>,
    /// When the prompt's answer grace runs out, once input armed it.
    pub(super) answer_grace_until: Option<Instant>,
    pub(super) action_steps: VecDeque<ActionStep>,
    /// Outputs Stillwatch blanked and hasn't woken yet.
    pub(super) blanked: Option<Vec<String>>,
    /// The method of the last blank sent.
    pub(super) blank_method: BlankMethod,
    /// Re-blanks since the last input or fresh blank.
    pub(super) reblank_attempts: u32,
    /// Between `PrepareForSleep` and `ResumedFromSleep`.
    pub(super) asleep: bool,
    /// Screen-on tracking. Disabled config freezes it.
    pub(super) panel: PanelTracker,
    /// Deadline of [`TimerId::PanelCareReminder`], if this step armed it.
    pub(super) panel_check: Option<Instant>,
    /// Timers armed and not yet fired or cancelled.
    armed: HashSet<TimerId>,
}

impl Ctx {
    pub(super) fn new(config: &Config, detector: Box<dyn StaleDetector>, now: Instant) -> Self {
        Self {
            config: config.clone(),
            detector,
            now,
            wall: Timestamp::UNIX_EPOCH,
            out: Vec::new(),
            idle: false,
            compositor: Compositor::Up,
            reconnects: 0,
            outputs: None,
            hotplug_wakes: HashMap::new(),
            blank_generation: HashMap::new(),
            locked: false,
            playing: Vec::new(),
            last_gamepad: None,
            last_detection: None,
            snooze_until: None,
            answer_grace_until: None,
            action_steps: VecDeque::new(),
            blanked: None,
            blank_method: config.action.blank_method,
            reblank_attempts: 0,
            asleep: false,
            panel: PanelTracker::new(config.panel_care.clone()),
            panel_check: None,
            armed: HashSet::new(),
        }
    }

    pub(super) fn begin(&mut self, now: Instant, wall: Timestamp) {
        self.now = now;
        self.wall = wall;
    }

    pub(super) fn emit(&mut self, command: Command) {
        self.out.push(command);
    }

    pub(super) fn take_commands(&mut self) -> Vec<Command> {
        std::mem::take(&mut self.out)
    }

    pub(super) fn set_timer(&mut self, id: TimerId, after: Duration) {
        self.armed.insert(id);
        self.emit(Command::SetTimer { id, after });
    }

    pub(super) fn cancel_timer(&mut self, id: TimerId) {
        self.armed.remove(&id);
        self.emit(Command::CancelTimer(id));
    }

    /// Cancels `id` only if it is still armed.
    pub(super) fn disarm(&mut self, id: TimerId) {
        if self.armed.contains(&id) {
            self.cancel_timer(id);
        }
    }

    pub(super) fn is_armed(&self, id: TimerId) -> bool {
        self.armed.contains(&id)
    }

    /// Notes that `id` fired, so it is no longer armed.
    pub(super) fn fired(&mut self, id: TimerId) {
        self.armed.remove(&id);
    }

    /// A history entry of `kind` stamped with the current time and context.
    pub(super) fn history(&self, kind: HistoryKind) -> HistoryEntry {
        HistoryEntry::new(self.wall, kind).with_context(self.decision_context())
    }

    pub(super) fn decision_context(&self) -> DecisionContext {
        DecisionContext {
            media_playing: self.media_playing(),
            gamepad_active: self.gamepad_active(),
            locked: self.locked,
        }
    }

    pub(super) fn media_playing(&self) -> bool {
        self.config.stale.media_playing(&self.playing)
    }

    fn gamepad_active(&self) -> bool {
        let window = Duration::from_mins(u64::from(self.config.idle.input_idle_minutes));
        self.last_gamepad
            .is_some_and(|at| self.now.saturating_duration_since(at) < window)
    }

    /// Where to go once a snooze or pause no longer holds the machine.
    /// A locked session goes through Active, which hands over to Locked.
    pub(super) const fn watch_or_active(&self) -> State {
        if self.idle && !self.locked {
            State::Monitoring
        } else {
            State::Active
        }
    }

    /// The compositor idle watch is up, so idle may blank.
    pub(super) const fn activity_known(&self) -> bool {
        matches!(self.compositor, Compositor::Up)
    }

    /// The idle watch is reporting again.
    pub(super) fn mark_activity_known(&mut self) {
        self.compositor = Compositor::Up;
    }

    pub(super) fn arm_capture(&mut self) {
        if self.captures_paused() {
            self.disarm(TimerId::Capture);
            return;
        }
        let every = Duration::from_secs(u64::from(self.config.stale.check_interval_seconds));
        self.set_timer(TimerId::Capture, every);
    }

    pub(super) fn request_capture(&mut self) {
        if self.captures_paused() {
            return;
        }
        self.emit(Command::RequestCapture {
            outputs: self.config.stale.monitored_outputs.clone(),
            downscale_width: self.config.stale.downscale_width,
        });
    }

    /// Runs the detector over a capture tick and remembers the verdict.
    pub(super) fn observe(&mut self, frames: &[CaptureFrame]) -> DetectionStats {
        let stats = self.detector.observe(frames, &self.playing);
        self.last_detection = Some(stats.clone());
        stats
    }

    pub(super) fn prompt_request(&self, detection: Option<&DetectionStats>) -> PromptRequest {
        let prompt = &self.config.prompt;
        PromptRequest {
            countdown: self.countdown(),
            presets: prompt_presets(prompt).collect(),
            allow_custom: prompt.allow_custom,
            stale_outputs: detection
                .map(StaleOutput::from_detection)
                .unwrap_or_default(),
        }
    }

    pub(super) fn countdown(&self) -> Duration {
        Duration::from_secs(u64::from(self.config.prompt.countdown_seconds))
    }

    /// A transition to Snoozed, if `duration` passes the snooze rules.
    pub(super) fn snooze(&self, duration: Duration) -> Option<Transition> {
        validate_snooze(&self.config.prompt, duration)
            .ok()
            .map(|duration| Transition::to(State::Snoozed).with_snooze(duration))
    }

    pub(super) fn action_outputs(&self) -> Vec<String> {
        match self.config.action.outputs {
            ActionOutputs::Monitored => self.config.stale.monitored_outputs.clone(),
            ActionOutputs::All => Vec::new(),
        }
    }

    /// Runs a hook if its command is configured.
    pub(super) fn hook(&mut self, kind: HookKind) {
        if !self.config.hook_command(kind).trim().is_empty() {
            self.emit(Command::RunHook(kind));
        }
    }

    /// Wakes outputs Stillwatch blanked, if any.
    pub(super) fn wake_displays(&mut self) {
        if let Some(outputs) = self.blanked.take() {
            self.emit(Command::Unblank { outputs });
            self.hook(HookKind::OnResume);
        }
    }
}
