//! Inputs to the state machine.
//!
//! Backends push events into an [`EventSink`](crate::backend::EventSink); the
//! daemon also turns command results, fired timers, and D-Bus calls into
//! events.

use std::time::Duration;

use crate::backend::BackendError;
use crate::luma::{LumaGrid, OutputInfo};
use crate::prompt::PromptOutcome;
use crate::time::TimerId;

/// Keyboard, mouse, and gamepad activity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivityEvent {
    /// The compositor reports no keyboard or mouse input for the idle timeout.
    InputIdle,
    /// Keyboard or mouse input after `InputIdle`.
    InputResumed,
    /// A gamepad event passed the deadzone.
    GamepadActivity {
        /// [`GamepadDevice::id`](crate::backend::GamepadDevice::id) of the device.
        device: String,
    },
}

/// Session lock and suspend notifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionEvent {
    /// The session was locked.
    Locked,
    /// The session was unlocked.
    Unlocked,
    /// The system is about to suspend.
    PrepareForSleep,
    /// The system resumed from suspend.
    ResumedFromSleep,
}

/// A control request from the CLI or GUI over D-Bus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlCommand {
    /// Snooze for this long.
    Snooze(Duration),
    /// End a snooze early.
    CancelSnooze,
    /// Pause Stillwatch until resumed.
    Pause,
    /// Undo a pause.
    Resume,
    /// Reload the config. The daemon handles the file and reports the result
    /// with [`Event::ConfigReloaded`].
    Reload,
}

/// One output's luma grid from a capture tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureFrame {
    /// Connector name, for example `HDMI-A-1`.
    pub output: String,
    /// The downscaled luma grid.
    pub grid: LumaGrid,
}

/// An input to the state machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Input activity or idleness.
    Activity(ActivityEvent),
    /// Session lock or suspend.
    Session(SessionEvent),
    /// An output's power state changed. `on: true` while blanked means the
    /// display woke (DPMS on, or the overlay surface went away).
    DisplayPower {
        /// Connector name.
        output: String,
        /// Whether the output is now showing content.
        on: bool,
    },
    /// The set of MPRIS players reporting `Playing` changed. Holds every
    /// playing player's name (as from `MediaWatcher::players`); the ignore
    /// list is applied by the consumer. Empty when nothing is playing.
    Media {
        /// Names of the players currently playing.
        playing: Vec<String>,
    },
    /// A capture tick finished for every requested output.
    CaptureCompleted {
        /// One frame per captured output.
        frames: Vec<CaptureFrame>,
    },
    /// A capture tick failed.
    CaptureFailed {
        /// Why.
        error: BackendError,
    },
    /// The prompt was answered (by the prompter or via D-Bus `PromptAnswer`).
    PromptAnswered(PromptOutcome),
    /// The prompt couldn't be shown or failed while showing.
    PromptFailed {
        /// Why.
        error: BackendError,
    },
    /// The last `Blank` or `Lock` command succeeded.
    ActionCompleted,
    /// The last `Blank` or `Lock` command failed.
    ActionFailed {
        /// Why.
        error: BackendError,
    },
    /// A control request from D-Bus.
    Control(ControlCommand),
    /// Outputs were connected or disconnected. Holds the full current list.
    OutputsChanged(Vec<OutputInfo>),
    /// A timer armed with `Command::SetTimer` fired.
    Timer(TimerId),
    /// A new config was loaded and handed to the state machine.
    ConfigReloaded,
}

impl From<ActivityEvent> for Event {
    fn from(event: ActivityEvent) -> Self {
        Self::Activity(event)
    }
}

impl From<SessionEvent> for Event {
    fn from(event: SessionEvent) -> Self {
        Self::Session(event)
    }
}

impl From<ControlCommand> for Event {
    fn from(command: ControlCommand) -> Self {
        Self::Control(command)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sub_events_convert_into_events() {
        assert_eq!(
            Event::from(ActivityEvent::InputIdle),
            Event::Activity(ActivityEvent::InputIdle)
        );
        assert_eq!(
            Event::from(SessionEvent::PrepareForSleep),
            Event::Session(SessionEvent::PrepareForSleep)
        );
        assert_eq!(
            Event::from(ControlCommand::Snooze(Duration::from_secs(60))),
            Event::Control(ControlCommand::Snooze(Duration::from_secs(60)))
        );
    }
}
