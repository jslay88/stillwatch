//! What the state machine asks a prompter to show, and what comes back.

use std::time::Duration;

/// A request to show the burn-in prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptRequest {
    /// How long until the action runs if nobody answers.
    pub countdown: Duration,
    /// Snooze presets offered as buttons, in display order.
    pub presets: Vec<Duration>,
    /// Whether to offer "Custom..." (which opens `stillwatch-gui prompt`).
    pub allow_custom: bool,
}

/// How a prompt ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptOutcome {
    /// The user snoozed for this long (a preset or a custom value).
    Snooze(Duration),
    /// The user picked "Custom...". The prompt stays logically open; the real
    /// answer arrives later through the D-Bus `PromptAnswer` method.
    CustomRequested,
    /// The user cancelled: they're here, don't act.
    Cancel,
    /// The prompter's own countdown ran out.
    Timeout,
    /// The prompt was closed without picking an action.
    Dismissed,
}

/// A non-blocking informational notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reminder {
    /// Screen-on time passed `reminder_hours`; turning the display off lets
    /// the panel's compensation cycle run.
    PanelCare {
        /// Accumulated screen-on time since the last long enough standby.
        screen_on: Duration,
    },
}
