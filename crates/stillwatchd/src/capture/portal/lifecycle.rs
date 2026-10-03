//! When the portal stream is allowed to run.
//!
//! The screen-sharing indicator is visible for the whole session, so the
//! stream starts only while the daemon wants captures (idle in Monitoring, or
//! idle in Snoozed or Paused while the ceiling applies) and stops as soon as
//! the user is active. Time comes from a [`Clock`](stillwatch_core::time::Clock),
//! so the back-off between a dropped session and the next attempt is tested
//! without sleeping.

use std::time::Duration;

use stillwatch_core::backend::BackendError;
use stillwatch_core::backoff::Backoff;
use stillwatch_core::state::State;
use stillwatch_core::time::Clock;

use super::error::blocks_capture;

/// Whether the user is away (captures wanted) or active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    /// Idle in a state that captures. The stream may run.
    Away,
    /// The user is present, or captures aren't wanted. The stream must be stopped.
    Active,
}

/// Whether the safety ceiling is capturing while snoozed or paused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CeilingCapture {
    /// `safety.ceiling_enabled`.
    pub enabled: bool,
    /// `safety.ceiling_during_pause`.
    pub during_pause: bool,
}

/// The facts that decide [`Presence`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureWant {
    /// The daemon's state.
    pub state: State,
    /// The compositor reports input idle and the gamepad is quiet.
    pub idle: bool,
    /// logind is preparing for sleep or the session is suspended.
    pub asleep: bool,
    /// Ceiling settings that keep capturing after a snooze or a pause.
    pub ceiling: CeilingCapture,
}

/// What the backend should do to the portal session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    /// Open a session and start `PipeWire`.
    Start,
    /// Close the session. The indicator goes away.
    Stop,
    /// Leave the session as it is.
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Run {
    Stopped,
    Starting,
    Streaming,
    Waiting,
}

/// Start on away, stop on active, and wait out back-off after a transient failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionMachine {
    presence: Presence,
    run: Run,
    backoff: Backoff,
    /// Denial or a missing portal. Stays until the process restarts.
    blocked: bool,
    since: Option<std::time::Instant>,
    until: Option<std::time::Instant>,
}

impl SessionMachine {
    /// Stopped, and not streaming.
    #[must_use]
    pub fn new() -> Self {
        Self {
            presence: Presence::Active,
            run: Run::Stopped,
            backoff: Backoff::default(),
            blocked: false,
            since: None,
            until: None,
        }
    }

    /// Whether a `PipeWire` stream is up.
    #[must_use]
    pub const fn streaming(&self) -> bool {
        matches!(self.run, Run::Streaming)
    }

    /// Whether a permanent failure has disabled portal capture.
    #[must_use]
    pub const fn blocked(&self) -> bool {
        self.blocked
    }

    /// Updates presence and returns the session change that should follow.
    pub fn set_presence(&mut self, presence: Presence, clock: &dyn Clock) -> Order {
        self.presence = presence;
        match presence {
            Presence::Active => self.stop_clean(),
            Presence::Away => self.start_if_allowed(clock),
        }
    }

    /// The session came up.
    pub fn started(&mut self, clock: &dyn Clock) {
        self.run = Run::Streaming;
        self.since = Some(clock.now());
        self.until = None;
    }

    /// The session failed. Permanent failures disable capture; transient ones
    /// arm back-off and ask the caller to close whatever is half-open.
    pub fn failed(&mut self, error: &BackendError, clock: &dyn Clock) -> Order {
        let ran = self.since.map_or(Duration::ZERO, |since| {
            clock.now().saturating_duration_since(since)
        });
        self.since = None;
        if blocks_capture(error) {
            self.blocked = true;
            self.run = Run::Stopped;
            self.until = None;
            return Order::Stop;
        }
        let delay = self.backoff.after_attempt(ran);
        self.run = Run::Waiting;
        self.until = clock.now().checked_add(delay);
        if self.presence == Presence::Active {
            self.run = Run::Stopped;
            self.until = None;
        }
        Order::Stop
    }

    /// How long until a retry, when one is waiting.
    #[must_use]
    pub fn retry_in(&self, clock: &dyn Clock) -> Option<Duration> {
        self.until
            .map(|until| until.saturating_duration_since(clock.now()))
    }

    /// Starts a retry once the back-off has elapsed and the user is still away.
    pub fn poll(&mut self, clock: &dyn Clock) -> Order {
        if self.presence != Presence::Away || self.blocked {
            return Order::None;
        }
        let ready = self.until.is_some_and(|until| clock.now() >= until);
        if self.run == Run::Waiting && ready {
            self.until = None;
            self.run = Run::Starting;
            self.since = Some(clock.now());
            return Order::Start;
        }
        Order::None
    }

    fn start_if_allowed(&mut self, clock: &dyn Clock) -> Order {
        if self.blocked || matches!(self.run, Run::Starting | Run::Streaming) {
            return Order::None;
        }
        if self.run == Run::Waiting {
            return self.poll(clock);
        }
        self.run = Run::Starting;
        self.since = Some(clock.now());
        Order::Start
    }

    fn stop_clean(&mut self) -> Order {
        let stop = matches!(self.run, Run::Starting | Run::Streaming);
        self.run = Run::Stopped;
        self.since = None;
        self.until = None;
        if !self.blocked {
            self.backoff.reset();
        }
        if stop { Order::Stop } else { Order::None }
    }
}

impl Default for SessionMachine {
    fn default() -> Self {
        Self::new()
    }
}

/// Away while idle in Monitoring, or while the snooze ceiling is capturing.
#[must_use]
pub fn presence_for(want: CaptureWant) -> Presence {
    if !want.idle || want.asleep {
        return Presence::Active;
    }
    let away = match want.state {
        State::Monitoring => true,
        State::Snoozed => want.ceiling.enabled,
        State::Paused => want.ceiling.enabled && want.ceiling.during_pause,
        State::Active | State::Prompting | State::Acting | State::Blanked | State::Locked => false,
    };
    if away {
        Presence::Away
    } else {
        Presence::Active
    }
}

#[cfg(test)]
mod tests;
