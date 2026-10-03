use std::time::{Duration, Instant};

use super::ScriptedDetector;
use crate::command::Command;
use crate::config::{Config, ConfigError, LoadOutcome};
use crate::event::{ActivityEvent, CaptureFrame, Event};
use crate::luma::LumaGrid;
use crate::prompt::PromptOutcome;
use crate::state::{State, StateMachine, StatusSnapshot};
use crate::time::{Clock, FakeClock, TimerId, TimerQueue};

/// Upper bound on timer firings per `advance`, so a machine that keeps
/// re-arming zero-length timers can't hang a test.
const MAX_FIRINGS: usize = 10_000;

/// Drives a [`StateMachine`] the way the daemon does, on a [`FakeClock`].
///
/// Every command the machine returns is logged, and `SetTimer`/`CancelTimer`
/// are applied to a [`TimerQueue`]. [`advance`](Self::advance) moves the
/// clock and fires due timers in deadline order, feeding each back as
/// `Event::Timer`. The machine owns a [`ScriptedDetector`]; script it through
/// [`detector`](Self::detector) or [`capture`](Self::capture).
pub struct Harness {
    clock: FakeClock,
    detector: ScriptedDetector,
    machine: StateMachine,
    timers: TimerQueue,
    log: Vec<Command>,
}

impl Harness {
    /// A harness with the default config, in Active.
    #[must_use]
    pub fn new() -> Self {
        Self::with_config(&Config::default())
    }

    /// A harness running `config`, in Active.
    #[must_use]
    pub fn with_config(config: &Config) -> Self {
        let clock = FakeClock::new();
        let detector = ScriptedDetector::new();
        let (machine, commands) =
            StateMachine::new(config, Box::new(detector.clone()), clock.now());
        let mut harness = Self {
            clock,
            detector,
            machine,
            timers: TimerQueue::new(),
            log: Vec::new(),
        };
        harness.absorb(&commands);
        harness
    }

    /// Feeds one event at the current time and returns the commands.
    pub fn send(&mut self, event: impl Into<Event>) -> Vec<Command> {
        let commands = self
            .machine
            .handle(self.clock.now(), self.clock.wall_now(), &event.into());
        self.absorb(&commands);
        commands
    }

    /// Moves the clock forward by `by`, firing every timer that comes due on
    /// the way. Returns the commands from all firings, in order.
    pub fn advance(&mut self, by: Duration) -> Vec<Command> {
        let target = self.clock.now().checked_add(by).unwrap_or(self.clock.now());
        let mut commands = Vec::new();
        for _ in 0..MAX_FIRINGS {
            let Some(deadline) = self.timers.next_deadline().filter(|at| *at <= target) else {
                break;
            };
            self.advance_clock_to(deadline);
            for id in self.timers.pop_due(deadline) {
                commands.extend(self.send(Event::Timer(id)));
            }
        }
        self.advance_clock_to(target);
        commands
    }

    /// Advances to `id`'s deadline, firing it and anything due before it.
    /// Does nothing when `id` isn't pending.
    pub fn fire(&mut self, id: TimerId) -> Vec<Command> {
        match self.timers.deadline(id) {
            Some(deadline) => self.advance(deadline.saturating_duration_since(self.clock.now())),
            None => Vec::new(),
        }
    }

    /// Hands the machine a new config.
    pub fn apply_config(&mut self, config: &Config) -> Vec<Command> {
        let commands = self
            .machine
            .apply_config(self.clock.now(), self.clock.wall_now(), config);
        self.absorb(&commands);
        commands
    }

    /// Reports a reload that failed with `error`.
    pub fn reload_failed(&mut self, error: &ConfigError) -> Vec<Command> {
        let commands =
            self.machine
                .config_reload_failed(self.clock.now(), self.clock.wall_now(), error);
        self.absorb(&commands);
        commands
    }

    /// Reports a successful load that may have been migrated.
    pub fn loaded(&mut self, outcome: &LoadOutcome) -> Vec<Command> {
        let commands =
            self.machine
                .config_migrated(self.clock.now(), self.clock.wall_now(), outcome);
        self.absorb(&commands);
        commands
    }

    /// The user went idle.
    pub fn idle(&mut self) -> Vec<Command> {
        self.send(ActivityEvent::InputIdle)
    }

    /// Keyboard or mouse input.
    pub fn input(&mut self) -> Vec<Command> {
        self.send(ActivityEvent::InputResumed)
    }

    /// A gamepad event past the deadzone.
    pub fn gamepad(&mut self) -> Vec<Command> {
        self.send(ActivityEvent::GamepadActivity {
            device: "pad0".into(),
        })
    }

    /// Completes a capture whose verdict is stale or fresh.
    pub fn capture(&mut self, stale: bool) -> Vec<Command> {
        self.detector.push_verdict(stale);
        self.complete_capture()
    }

    /// Completes a capture with one `HDMI-A-1` frame, leaving the verdict to
    /// whatever the detector has queued.
    pub fn complete_capture(&mut self) -> Vec<Command> {
        let frames = LumaGrid::filled(4, 4, 128)
            .ok()
            .map(|grid| CaptureFrame {
                output: "HDMI-A-1".into(),
                grid,
            })
            .into_iter()
            .collect();
        self.send(Event::CaptureCompleted { frames })
    }

    /// Answers the prompt.
    pub fn answer(&mut self, outcome: PromptOutcome) -> Vec<Command> {
        self.send(Event::PromptAnswered(outcome))
    }

    /// Active -> Monitoring -> Prompting via an idle report and a stale capture.
    pub fn to_prompting(&mut self) -> Vec<Command> {
        let mut commands = self.idle();
        commands.extend(self.capture(true));
        commands
    }

    /// Runs the default path to Blanked: prompt, countdown timeout, action completed.
    pub fn to_blanked(&mut self) -> Vec<Command> {
        let mut commands = self.to_prompting();
        commands.extend(self.fire(TimerId::PromptCountdown));
        while self.state() == State::Acting {
            commands.extend(self.send(Event::ActionCompleted));
        }
        commands
    }

    /// The machine's current state.
    #[must_use]
    pub const fn state(&self) -> State {
        self.machine.state()
    }

    /// The machine's status at the current time.
    #[must_use]
    pub fn status(&self) -> StatusSnapshot {
        self.machine.status(self.clock.now())
    }

    /// The machine under test.
    #[must_use]
    pub const fn machine(&self) -> &StateMachine {
        &self.machine
    }

    /// The machine's detector.
    #[must_use]
    pub const fn detector(&self) -> &ScriptedDetector {
        &self.detector
    }

    /// The shared fake clock.
    #[must_use]
    pub const fn clock(&self) -> &FakeClock {
        &self.clock
    }

    /// Pending timers.
    #[must_use]
    pub const fn timers(&self) -> &TimerQueue {
        &self.timers
    }

    /// Time left on `id`, if pending.
    #[must_use]
    pub fn remaining(&self, id: TimerId) -> Option<Duration> {
        self.timers
            .deadline(id)
            .map(|at| at.saturating_duration_since(self.clock.now()))
    }

    /// Every command returned so far, oldest first.
    #[must_use]
    pub fn log(&self) -> &[Command] {
        &self.log
    }

    /// Every `(from, to)` state change so far, oldest first.
    #[must_use]
    pub fn transitions(&self) -> Vec<(State, State)> {
        self.log
            .iter()
            .filter_map(|command| match command {
                Command::StateChanged { from, to } => Some((*from, *to)),
                _ => None,
            })
            .collect()
    }

    fn absorb(&mut self, commands: &[Command]) {
        let now = self.clock.now();
        for command in commands {
            self.timers.apply(now, command);
        }
        self.log.extend_from_slice(commands);
    }

    fn advance_clock_to(&self, at: Instant) {
        self.clock
            .advance(at.saturating_duration_since(self.clock.now()));
    }
}

impl Default for Harness {
    fn default() -> Self {
        Self::new()
    }
}
