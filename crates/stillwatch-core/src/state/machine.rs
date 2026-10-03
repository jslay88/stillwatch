use std::time::{Duration, Instant};

use jiff::Timestamp;

use super::State;
use super::care;
use super::context::{Ctx, Transition};
use super::detector::StaleDetector;
use super::handlers::{common, handler};
use super::snooze::{SnoozeError, validate_snooze};
use super::status::StatusSnapshot;
use super::transitions::rule_for;
use crate::command::Command;
use crate::config::{Config, ConfigError, LoadOutcome};
use crate::event::Event;
use crate::history::HistoryKind;
use crate::panel::PanelRecord;

/// The Stillwatch state machine: events in, commands out.
///
/// It never sleeps or touches I/O. The daemon feeds it [`Event`]s with the
/// current monotonic and wall time and executes the [`Command`]s it returns,
/// including `SetTimer`/`CancelTimer`, whose firings come back as
/// `Event::Timer`.
///
/// `Event::Activity(InputIdle)` must mean *aggregated* idle: the compositor
/// reports input idle and no gamepad event passed the deadzone for
/// `idle.input_idle_minutes`. Gamepad activity counts as input, so the
/// aggregator sends `InputIdle` again once the gamepad goes quiet.
pub struct StateMachine {
    state: State,
    entered_at: Instant,
    ctx: Ctx,
}

impl StateMachine {
    /// A machine in [`State::Active`] that owns `detector`.
    ///
    /// The returned commands are the start-up effects to execute.
    #[must_use]
    pub fn new(
        config: &Config,
        detector: Box<dyn StaleDetector>,
        now: Instant,
    ) -> (Self, Vec<Command>) {
        let machine = Self {
            state: State::Active,
            entered_at: now,
            ctx: Ctx::new(config, detector, now),
        };
        (machine, Vec::new())
    }

    /// Handles one event and returns the commands to execute, in order.
    ///
    /// `Event::ConfigReloaded` is ignored here because it carries no config;
    /// hand the new config to [`apply_config`](Self::apply_config) instead.
    pub fn handle(&mut self, now: Instant, wall: Timestamp, event: &Event) -> Vec<Command> {
        self.ctx.begin(now, wall);
        if common::observe(&mut self.ctx, event) {
            let next = common::global(self.state, &mut self.ctx, event)
                .or_else(|| handler(self.state).on_event(&mut self.ctx, event));
            if let Some(next) = next {
                self.go(next);
            }
        }
        care::after_event(&mut self.ctx, event);
        self.ctx.take_commands()
    }

    /// Switches to a newly loaded config and records the reload.
    ///
    /// Timers already armed keep their deadlines, except the capture timer,
    /// which is re-armed with the new interval while capturing (or started
    /// or stopped when the ceiling settings change), and the locked blank
    /// delay, which starts or stops with `session.when_locked`. The detector
    /// gets the config too ([`StaleDetector::apply_config`]): it keeps its
    /// block counters unless a key that resets detection changed, so a
    /// reload never needs [`replace_detector`](Self::replace_detector).
    pub fn apply_config(&mut self, now: Instant, wall: Timestamp, config: &Config) -> Vec<Command> {
        self.ctx.begin(now, wall);
        self.ctx.config = config.clone();
        self.ctx.detector.apply_config(config);
        care::reconfigure(&mut self.ctx);
        handler(self.state).reconfigure(&mut self.ctx);
        let entry = self.ctx.history(HistoryKind::ConfigReload);
        self.ctx.emit(Command::Record(entry));
        self.ctx.take_commands()
    }

    /// Records a reload that failed, while the current config stays in effect.
    ///
    /// Call it whenever the daemon rejects a new config file (parse,
    /// migration, or validation errors). Only the number of problems is
    /// recorded, never the messages, because they can contain paths.
    pub fn config_reload_failed(
        &mut self,
        now: Instant,
        wall: Timestamp,
        error: &ConfigError,
    ) -> Vec<Command> {
        self.ctx.begin(now, wall);
        let entry = self
            .ctx
            .history(HistoryKind::ConfigReloadFailed)
            .with_error_count(problem_count(error));
        self.ctx.emit(Command::Record(entry));
        self.ctx.take_commands()
    }

    /// Records that `outcome` was migrated in memory from an older version.
    /// Returns nothing when it wasn't.
    ///
    /// Call it after every successful load, at start-up (after
    /// [`new`](Self::new)) and on reload (after
    /// [`apply_config`](Self::apply_config)).
    pub fn config_migrated(
        &mut self,
        now: Instant,
        wall: Timestamp,
        outcome: &LoadOutcome,
    ) -> Vec<Command> {
        let Some(from) = outcome.migrated_from else {
            return Vec::new();
        };
        self.ctx.begin(now, wall);
        let entry = self
            .ctx
            .history(HistoryKind::Migration)
            .with_versions(from, outcome.config.version);
        self.ctx.emit(Command::Record(entry));
        self.ctx.take_commands()
    }

    /// Swaps in a different detector, which starts with no history and no
    /// output sizes until the next `Event::OutputsChanged`. Reloads and
    /// hotplug don't need this: [`apply_config`](Self::apply_config) and
    /// `Event::OutputsChanged` reach the current detector.
    pub fn replace_detector(&mut self, detector: Box<dyn StaleDetector>) {
        self.ctx.detector = detector;
    }

    /// The current state.
    #[must_use]
    pub const fn state(&self) -> State {
        self.state
    }

    /// The config in effect.
    #[must_use]
    pub const fn config(&self) -> &Config {
        &self.ctx.config
    }

    /// Checks a snooze duration against the current `[prompt]` rules, so a
    /// D-Bus caller can get an error before the machine silently ignores it.
    ///
    /// # Errors
    ///
    /// Returns the [`SnoozeError`] describing the rule `duration` breaks.
    pub fn validate_snooze(&self, duration: Duration) -> Result<Duration, SnoozeError> {
        validate_snooze(&self.ctx.config.prompt, duration)
    }

    /// A status report as of `now`.
    #[must_use]
    pub fn status(&self, now: Instant) -> StatusSnapshot {
        let ctx = &self.ctx;
        StatusSnapshot {
            state: self.state,
            in_state: now.saturating_duration_since(self.entered_at),
            snooze_remaining: ctx
                .snooze_until
                .map(|until| until.saturating_duration_since(now)),
            idle: ctx.idle,
            locked: ctx.locked,
            media_playing: ctx.media_playing(),
            last_detection: ctx.last_detection.clone(),
        }
    }

    /// Whether outputs Stillwatch blanked are still waiting for an unblank.
    #[must_use]
    pub const fn displays_blanked(&self) -> bool {
        self.ctx.blanked.is_some()
    }

    /// Panel care counters when tracking is enabled.
    ///
    /// `None` when `panel_care.enabled` is false, so status omits the section.
    #[must_use]
    pub fn panel_record(&self, now: Instant) -> Option<PanelRecord> {
        care::record(&self.ctx, now)
    }

    /// Continues from counters loaded out of `panel.json`.
    pub fn restore_panel(&mut self, record: PanelRecord) {
        care::restore(&mut self.ctx, record);
    }

    /// Takes `next` and any follow-ups its `enter` hook returns. Transitions
    /// missing from [`TRANSITIONS`](super::TRANSITIONS) are refused.
    pub(super) fn go(&mut self, mut next: Transition) {
        // Every hop must change state, so a chain longer than the number of
        // states can only be a loop.
        for _ in 0..State::ALL.len() {
            let from = self.state;
            if rule_for(from, next.to).is_none() {
                return;
            }
            handler(from).exit(&mut self.ctx);
            self.state = next.to;
            self.entered_at = self.ctx.now;
            self.ctx.emit(Command::StateChanged { from, to: next.to });
            let entry = next
                .history_entry(self.ctx.wall, from)
                .with_context(self.ctx.decision_context());
            self.ctx.emit(Command::Record(entry));
            match handler(next.to).enter(&mut self.ctx, &next) {
                Some(follow_up) => next = follow_up,
                None => return,
            }
        }
    }
}

/// Validation failures report every broken rule; anything else is one problem.
fn problem_count(error: &ConfigError) -> u32 {
    match error {
        ConfigError::Invalid(issues) => u32::try_from(issues.len()).unwrap_or(u32::MAX),
        _ => 1,
    }
}
