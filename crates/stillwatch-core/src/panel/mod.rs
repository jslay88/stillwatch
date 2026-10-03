//! Screen-on tracking so a panel's own compensation cycle can run.
//!
//! Time comes from a [`Clock`]: screen-on seconds accumulate while any known
//! output is on, including under the black overlay. The total resets only
//! after every known output has been in real standby (DPMS or DDC) for
//! [`PanelCareConfig::min_standby_minutes`](crate::config::PanelCareConfig::min_standby_minutes).
//! `panel_care.enabled = false` freezes the counters and suppresses the
//! reminder and the blank-time trigger.
//!
//! The reminder is sent once screen-on time exceeds `reminder_hours`, then
//! held for that same length of time. A qualifying standby clears the hold,
//! so the next time the total exceeds the threshold the reminder can fire
//! again. `trigger_cmd` is due on the same threshold, whether or not the
//! reminder itself is enabled.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};

use crate::config::PanelCareConfig;
use crate::event::PowerKind;
use crate::time::Clock;

#[cfg(test)]
mod tests;

/// Counters stored in `panel.json` and shown on status.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelRecord {
    /// Screen-on seconds since the last standby that reset the counter.
    pub screen_on_seconds: u64,
    /// Wall time when that standby reached `min_standby_minutes`.
    pub last_standby: Option<Timestamp>,
    /// Times an output was covered by the overlay instead of real standby.
    pub overlay_uses: u32,
}

/// What changed on one tracker step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelUpdate {
    /// Screen-on time for a reminder that should be sent now.
    pub reminder: Option<Duration>,
    /// When to look again. `None` means nothing is pending.
    pub check_after: Option<Duration>,
}

impl PanelUpdate {
    const fn idle() -> Self {
        Self {
            reminder: None,
            check_after: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputState {
    On,
    Overlay,
    Standby,
}

/// Accumulates screen-on time from power reports.
#[derive(Debug, Clone)]
pub struct PanelTracker {
    config: PanelCareConfig,
    outputs: BTreeMap<String, OutputState>,
    /// Closed screen-on time. The open interval is `on_since`.
    closed: Duration,
    on_since: Option<Instant>,
    standby_since: Option<Instant>,
    /// The current standby already reset the counter.
    standby_reset: bool,
    last_standby: Option<Timestamp>,
    overlay_uses: u32,
    /// Don't remind again until this instant.
    snooze_until: Option<Instant>,
    /// A snooze end that doesn't fit in `Instant`. Hold until the counter resets.
    reminder_held: bool,
}

impl PanelTracker {
    /// A tracker with empty counters, following `config`.
    #[must_use]
    pub fn new(config: PanelCareConfig) -> Self {
        Self {
            config,
            outputs: BTreeMap::new(),
            closed: Duration::ZERO,
            on_since: None,
            standby_since: None,
            standby_reset: false,
            last_standby: None,
            overlay_uses: 0,
            snooze_until: None,
            reminder_held: false,
        }
    }

    /// A tracker that continues from `saved`.
    #[must_use]
    pub fn restore(config: PanelCareConfig, saved: PanelRecord) -> Self {
        Self {
            closed: Duration::from_secs(saved.screen_on_seconds),
            last_standby: saved.last_standby,
            overlay_uses: saved.overlay_uses,
            ..Self::new(config)
        }
    }

    /// Replaces the saved counters, keeping the current config.
    pub fn restore_record(&mut self, saved: PanelRecord) {
        let config = self.config.clone();
        *self = Self::restore(config, saved);
    }

    /// Whether tracking, the reminder, and the trigger are active.
    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.config.enabled
    }

    /// Screen-on time as of `clock`, including the current on stretch.
    #[must_use]
    pub fn screen_on(&self, clock: &dyn Clock) -> Duration {
        let open = self.on_since.map_or(Duration::ZERO, |at| {
            clock.now().saturating_duration_since(at)
        });
        self.closed.saturating_add(open)
    }

    /// When the counter last reset, if it ever has.
    #[must_use]
    pub const fn last_standby(&self) -> Option<Timestamp> {
        self.last_standby
    }

    /// Overlay uses so far.
    #[must_use]
    pub const fn overlay_uses(&self) -> u32 {
        self.overlay_uses
    }

    /// The counters as of `clock`, for `panel.json` and status.
    #[must_use]
    pub fn record(&self, clock: &dyn Clock) -> PanelRecord {
        PanelRecord {
            screen_on_seconds: self.screen_on(clock).as_secs(),
            last_standby: self.last_standby,
            overlay_uses: self.overlay_uses,
        }
    }

    /// Whether `trigger_cmd` should run: tracking is on and screen-on time is
    /// over `reminder_hours`.
    #[must_use]
    pub fn trigger_due(&self, clock: &dyn Clock) -> bool {
        self.config.enabled && self.over_reminder(clock)
    }

    /// Notes one output's power and returns the reminder and the next check.
    pub fn power(
        &mut self,
        clock: &dyn Clock,
        output: &str,
        kind: PowerKind,
        on: bool,
    ) -> PanelUpdate {
        self.record_power(clock, output, kind, on);
        self.finish(clock)
    }

    /// Updates power without deciding a reminder. Used while the machine is
    /// asleep, when timers must stay disarmed; [`tick`](Self::tick) on resume
    /// sends a reminder that became due.
    pub fn record_power(&mut self, clock: &dyn Clock, output: &str, kind: PowerKind, on: bool) {
        if !self.config.enabled {
            return;
        }
        let next = output_state(kind, on);
        let prev = self.outputs.get(output).copied();
        if prev == Some(next) {
            return;
        }
        if next == OutputState::Overlay {
            self.overlay_uses = self.overlay_uses.saturating_add(1);
        }
        self.outputs.insert(output.to_owned(), next);
        self.sync_phase(clock);
    }

    /// Applies a standby reset or a reminder that came due since the last step.
    pub fn tick(&mut self, clock: &dyn Clock) -> PanelUpdate {
        self.finish(clock)
    }

    /// Switches to `config`. Disabling freezes the counters.
    pub fn set_config(&mut self, config: PanelCareConfig, clock: &dyn Clock) -> PanelUpdate {
        let was_enabled = self.config.enabled;
        self.config = config;
        if !self.config.enabled {
            if was_enabled {
                self.close_on(clock.now());
            }
            self.standby_since = None;
            self.standby_reset = false;
            return PanelUpdate::idle();
        }
        if !was_enabled {
            self.sync_phase(clock);
        }
        self.finish(clock)
    }

    fn finish(&mut self, clock: &dyn Clock) -> PanelUpdate {
        if !self.config.enabled {
            return PanelUpdate::idle();
        }
        self.apply_standby_reset(clock);
        PanelUpdate {
            reminder: self.maybe_remind(clock),
            check_after: self.check_after(clock),
        }
    }

    fn sync_phase(&mut self, clock: &dyn Clock) {
        if self.outputs.is_empty() {
            return;
        }
        if self
            .outputs
            .values()
            .any(|state| *state != OutputState::Standby)
        {
            self.enter_on(clock.now());
        } else {
            self.enter_standby(clock);
        }
    }

    fn enter_on(&mut self, now: Instant) {
        self.standby_since = None;
        self.standby_reset = false;
        if self.on_since.is_none() {
            self.on_since = Some(now);
        }
    }

    fn enter_standby(&mut self, clock: &dyn Clock) {
        self.close_on(clock.now());
        if self.standby_since.is_none() {
            self.standby_since = Some(clock.now());
            self.standby_reset = false;
        }
        self.apply_standby_reset(clock);
    }

    fn close_on(&mut self, now: Instant) {
        if let Some(at) = self.on_since.take() {
            self.closed = self
                .closed
                .saturating_add(now.saturating_duration_since(at));
        }
    }

    fn apply_standby_reset(&mut self, clock: &dyn Clock) {
        if self.standby_reset {
            return;
        }
        let Some(since) = self.standby_since else {
            return;
        };
        let now = clock.now();
        let limit = minutes(self.config.min_standby_minutes);
        let elapsed = now.saturating_duration_since(since);
        if elapsed < limit {
            return;
        }
        self.closed = Duration::ZERO;
        self.on_since = None;
        self.last_standby = Some(qualified_at(since, now, clock.wall_now(), limit));
        self.standby_reset = true;
        self.snooze_until = None;
        self.reminder_held = false;
    }

    fn maybe_remind(&mut self, clock: &dyn Clock) -> Option<Duration> {
        if !self.config.reminder_enabled || self.reminder_held || !self.over_reminder(clock) {
            return None;
        }
        let now = clock.now();
        if self.snooze_until.is_some_and(|until| now < until) {
            return None;
        }
        let screen_on = self.screen_on(clock);
        match now.checked_add(hours(self.config.reminder_hours)) {
            Some(until) => self.snooze_until = Some(until),
            None => self.reminder_held = true,
        }
        Some(screen_on)
    }

    fn over_reminder(&self, clock: &dyn Clock) -> bool {
        self.screen_on(clock) > hours(self.config.reminder_hours)
    }

    fn check_after(&self, clock: &dyn Clock) -> Option<Duration> {
        let now = clock.now();
        let mut waits = Vec::new();
        if let Some(since) = self.standby_since.filter(|_| !self.standby_reset) {
            let elapsed = now.saturating_duration_since(since);
            let limit = minutes(self.config.min_standby_minutes);
            if let Some(left) = limit.checked_sub(elapsed) {
                waits.push(left);
            }
        }
        if self.config.reminder_enabled && !self.reminder_held {
            if let Some(until) = self.snooze_until.filter(|until| *until > now) {
                waits.push(until.saturating_duration_since(now));
            } else if self.on_since.is_some() {
                let screen_on = self.screen_on(clock);
                let limit = hours(self.config.reminder_hours);
                if screen_on <= limit {
                    // One second past the threshold, so "exceeds" is true.
                    waits.push(limit.saturating_sub(screen_on) + Duration::from_secs(1));
                }
            }
        }
        waits.into_iter().min()
    }
}

fn output_state(kind: PowerKind, on: bool) -> OutputState {
    if on {
        OutputState::On
    } else if kind == PowerKind::Overlay {
        OutputState::Overlay
    } else {
        OutputState::Standby
    }
}

fn hours(value: u32) -> Duration {
    Duration::from_hours(u64::from(value))
}

fn minutes(value: u32) -> Duration {
    Duration::from_mins(u64::from(value))
}

/// Wall time when `since` had been going for `limit`, even if we noticed later.
fn qualified_at(since: Instant, now: Instant, wall: Timestamp, limit: Duration) -> Timestamp {
    let late = now.saturating_duration_since(since).saturating_sub(limit);
    let nanos = i64::try_from(late.as_nanos()).unwrap_or(i64::MAX);
    wall.checked_sub(SignedDuration::from_nanos(nanos))
        .unwrap_or(wall)
}
