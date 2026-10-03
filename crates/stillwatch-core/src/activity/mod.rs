//! Combines compositor input idle and gamepad activity into the one activity
//! stream the state machine consumes.
//!
//! The compositor's input idle never sees evdev gamepads, so on its own it
//! would call someone playing with a controller idle. [`ActivityAggregator`]
//! sits between the idle and gamepad sources and the
//! [`StateMachine`](crate::state::StateMachine): it sends `InputIdle` only
//! once the compositor is idle *and* no gamepad input passed the deadzone for
//! `idle.input_idle_minutes`. Keyboard and mouse resumes and every gamepad
//! event go through unchanged, so the machine applies its own per-state input
//! rules (including `gamepad_wakes_display`).
//!
//! Like the state machine it's sans-IO. It asks for its one timer with
//! [`Command::SetTimer`] and expects the firing back as
//! [`RawActivity::GamepadIdle`].

use std::time::{Duration, Instant};

use crate::command::Command;
use crate::config::Config;
use crate::event::{ActivityEvent, Event};
use crate::time::TimerId;

/// The config the aggregator works from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivitySettings {
    /// How long keyboard, mouse, and gamepad input must all be quiet
    /// (`idle.input_idle_minutes`). The compositor's idle watch should use
    /// the same timeout.
    pub input_idle: Duration,
    /// Whether gamepad input keeps the user active (`activity.gamepad`).
    pub gamepad: bool,
}

impl From<&Config> for ActivitySettings {
    fn from(config: &Config) -> Self {
        Self {
            input_idle: Duration::from_mins(u64::from(config.idle.input_idle_minutes)),
            gamepad: config.activity.gamepad,
        }
    }
}

impl Default for ActivitySettings {
    fn default() -> Self {
        Self::from(&Config::default())
    }
}

/// What the idle and gamepad sources report, before aggregation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawActivity {
    /// The compositor reports no keyboard or mouse input for the timeout.
    CompositorIdle,
    /// Keyboard or mouse input after `CompositorIdle`.
    CompositorResumed,
    /// The compositor idle watch was (re)created, after a reconnect or a
    /// timeout change. The new notification starts out not idle with a fresh
    /// countdown, and no `CompositorResumed` follows an earlier
    /// `CompositorIdle`, so the compositor state is unknown until it reports
    /// idle again. Unknown counts as active.
    WatchRestarted,
    /// A gamepad event passed the deadzone.
    Gamepad {
        /// [`GamepadDevice::id`](crate::backend::GamepadDevice::id) of the device.
        device: String,
    },
    /// The [`TimerId::GamepadIdle`] timer fired.
    GamepadIdle,
}

impl RawActivity {
    /// The raw activity behind an idle or gamepad source event, or a fired
    /// [`TimerId::GamepadIdle`]. `None` for every other event, which goes
    /// straight to the state machine.
    #[must_use]
    pub fn from_event(event: &Event) -> Option<Self> {
        match event {
            Event::Activity(ActivityEvent::InputIdle) => Some(Self::CompositorIdle),
            Event::Activity(ActivityEvent::InputResumed) => Some(Self::CompositorResumed),
            Event::Activity(ActivityEvent::GamepadActivity { device }) => Some(Self::Gamepad {
                device: device.clone(),
            }),
            Event::Timer(TimerId::GamepadIdle) => Some(Self::GamepadIdle),
            _ => None,
        }
    }
}

/// Why the user stopped counting as idle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WakeSource {
    /// The compositor saw keyboard or mouse input.
    KeyboardMouse,
    /// A gamepad passed the deadzone.
    Gamepad {
        /// [`GamepadDevice::id`](crate::backend::GamepadDevice::id) of the device.
        device: String,
    },
    /// The compositor idle watch restarted, so its state is unknown.
    WatchRestarted,
}

/// The combined idle state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Presence {
    /// The compositor is idle and gamepads have been quiet for the timeout.
    Idle,
    /// The user is back, and why.
    Active(WakeSource),
}

/// One thing for the caller to do after an aggregator step.
#[derive(Debug, Clone, PartialEq)]
pub enum ActivityOutput {
    /// Feed this to the state machine as `Event::Activity`.
    Machine(ActivityEvent),
    /// The combined state changed. For logs, decision history, and
    /// `stillwatch idle-test`.
    Changed(Presence),
    /// A `SetTimer` or `CancelTimer` for [`TimerId::GamepadIdle`]. Apply it
    /// to a [`TimerQueue`](crate::time::TimerQueue) and feed the firing back
    /// as [`RawActivity::GamepadIdle`].
    Timer(Command),
}

/// Turns raw compositor and gamepad activity into the combined stream the
/// state machine expects.
///
/// It starts out active, like the machine. Every `GamepadActivity` it
/// forwards makes the machine non-idle, and the aggregator tracks that, so it
/// sends `InputIdle` again once the gamepad has been quiet for the timeout.
#[derive(Debug, Clone)]
pub struct ActivityAggregator {
    settings: ActivitySettings,
    compositor_idle: bool,
    last_gamepad: Option<Instant>,
    idle: bool,
    timer_armed: bool,
}

impl ActivityAggregator {
    /// An aggregator in the active state.
    #[must_use]
    pub const fn new(settings: ActivitySettings) -> Self {
        Self {
            settings,
            compositor_idle: false,
            last_gamepad: None,
            idle: false,
            timer_armed: false,
        }
    }

    /// Handles one raw input at `now` and returns what to do, in order.
    pub fn handle(&mut self, now: Instant, raw: RawActivity) -> Vec<ActivityOutput> {
        let mut out = Vec::new();
        match raw {
            RawActivity::CompositorIdle => self.compositor_idle = true,
            RawActivity::CompositorResumed => {
                self.compositor_idle = false;
                // Forwarded even when already active: it's the keyboard or
                // mouse input that wakes displays a gamepad left blanked.
                out.push(ActivityOutput::Machine(ActivityEvent::InputResumed));
                self.wake(WakeSource::KeyboardMouse, &mut out);
            }
            RawActivity::WatchRestarted => {
                self.compositor_idle = false;
                if self.idle {
                    out.push(ActivityOutput::Machine(ActivityEvent::InputResumed));
                }
                self.wake(WakeSource::WatchRestarted, &mut out);
            }
            RawActivity::Gamepad { device } => {
                out.push(ActivityOutput::Machine(ActivityEvent::GamepadActivity {
                    device: device.clone(),
                }));
                if self.settings.gamepad {
                    self.last_gamepad = Some(now);
                    self.wake(WakeSource::Gamepad { device }, &mut out);
                }
            }
            RawActivity::GamepadIdle => self.timer_armed = false,
        }
        self.settle(now, &mut out);
        out
    }

    /// Switches to reloaded settings. A shorter timeout (or turning gamepads
    /// off) can make the user idle right away. Already idle stays idle, even
    /// if the new timeout is longer.
    pub fn apply_settings(
        &mut self,
        now: Instant,
        settings: ActivitySettings,
    ) -> Vec<ActivityOutput> {
        self.settings = settings;
        let mut out = Vec::new();
        self.settle(now, &mut out);
        out
    }

    /// Whether the combined state is idle.
    #[must_use]
    pub const fn is_idle(&self) -> bool {
        self.idle
    }

    /// The settings in effect.
    #[must_use]
    pub const fn settings(&self) -> &ActivitySettings {
        &self.settings
    }

    fn wake(&mut self, source: WakeSource, out: &mut Vec<ActivityOutput>) {
        if std::mem::replace(&mut self.idle, false) {
            out.push(ActivityOutput::Changed(Presence::Active(source)));
        }
    }

    /// Goes idle if both conditions hold, otherwise keeps the gamepad timer
    /// armed only while the compositor is idle and a pad is the holdout.
    fn settle(&mut self, now: Instant, out: &mut Vec<ActivityOutput>) {
        if self.idle || !self.compositor_idle {
            self.disarm(out);
            return;
        }
        let remaining = self.gamepad_quiet_in(now);
        if remaining.is_zero() {
            self.disarm(out);
            self.idle = true;
            out.push(ActivityOutput::Machine(ActivityEvent::InputIdle));
            out.push(ActivityOutput::Changed(Presence::Idle));
        } else {
            self.timer_armed = true;
            out.push(ActivityOutput::Timer(Command::SetTimer {
                id: TimerId::GamepadIdle,
                after: remaining,
            }));
        }
    }

    fn disarm(&mut self, out: &mut Vec<ActivityOutput>) {
        if std::mem::replace(&mut self.timer_armed, false) {
            out.push(ActivityOutput::Timer(Command::CancelTimer(
                TimerId::GamepadIdle,
            )));
        }
    }

    /// How long until gamepads have been quiet for the timeout; zero once
    /// they have.
    fn gamepad_quiet_in(&self, now: Instant) -> Duration {
        match self.last_gamepad {
            Some(at) if self.settings.gamepad => self
                .settings
                .input_idle
                .saturating_sub(now.saturating_duration_since(at)),
            _ => Duration::ZERO,
        }
    }
}

#[cfg(test)]
mod tests;
