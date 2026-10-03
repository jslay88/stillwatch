//! Effects the state machine asks the daemon to perform.
//!
//! The state machine returns `Vec<Command>` from each step. The daemon runs
//! each command against a backend and feeds any result back as an
//! [`Event`](crate::event::Event).

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::history::HistoryEntry;
use crate::prompt::{PromptRequest, Reminder};
use crate::state::State;
use crate::time::TimerId;

/// How displays are blanked.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlankMethod {
    /// Signal off through the compositor (lets the panel reach standby).
    #[default]
    Dpms,
    /// A black layer-shell surface per output (keeps the panel on).
    Overlay,
    /// MCCS power mode `0xD6` over DDC/CI.
    DdcStandby,
}

/// A user-configured shell command the daemon runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookKind {
    /// `action.on_blank_cmd`, after displays are blanked.
    OnBlank,
    /// `action.on_resume_cmd`, after displays wake.
    OnResume,
    /// `panel_care.trigger_cmd`, at blank time when panel care is due.
    PanelCareTrigger,
    /// `action.command`, when `action.mode = "command"`.
    ActionCommand,
}

/// An effect for the daemon to execute.
///
/// Output lists are connector names; an empty list means every connected
/// output.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Arm timer `id` to fire `after` from now, replacing a pending one with
    /// the same id. Fires back as `Event::Timer(id)`.
    SetTimer {
        /// The timer to arm.
        id: TimerId,
        /// Delay from now.
        after: Duration,
    },
    /// Disarm timer `id` if pending. A cancelled timer never fires.
    CancelTimer(TimerId),
    /// Capture each output once and reply with `Event::CaptureCompleted` or
    /// `Event::CaptureFailed`.
    RequestCapture {
        /// Outputs to capture.
        outputs: Vec<String>,
        /// Width of the luma grid to produce.
        downscale_width: u32,
    },
    /// Show the burn-in prompt. The answer comes back as
    /// `Event::PromptAnswered` or `Event::PromptFailed`.
    ShowPrompt(PromptRequest),
    /// Close the prompt if it's still showing.
    DismissPrompt,
    /// Blank outputs, replying with `Event::ActionCompleted` or
    /// `Event::ActionFailed`.
    Blank {
        /// Outputs to blank.
        outputs: Vec<String>,
        /// Which blanker to use.
        method: BlankMethod,
    },
    /// Wake outputs (undo a blank), for example when gamepad input arrives
    /// while blanked.
    Unblank {
        /// Outputs to wake.
        outputs: Vec<String>,
    },
    /// Lock the session, replying with `Event::ActionCompleted` or
    /// `Event::ActionFailed`.
    Lock,
    /// Run a configured hook command. Fire and forget.
    RunHook(HookKind),
    /// Append an entry to the decision history.
    Record(HistoryEntry),
    /// Show an informational reminder.
    Notify(Reminder),
    /// The state changed; the daemon emits the D-Bus `StateChanged` signal.
    StateChanged {
        /// Previous state.
        from: State,
        /// New state.
        to: State,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enums_serialize_as_snake_case() {
        assert_eq!(
            serde_json::to_string(&BlankMethod::DdcStandby).unwrap(),
            r#""ddc_standby""#
        );
        assert_eq!(
            serde_json::to_string(&HookKind::PanelCareTrigger).unwrap(),
            r#""panel_care_trigger""#
        );
        assert_eq!(
            serde_json::from_str::<BlankMethod>(r#""overlay""#).unwrap(),
            BlankMethod::Overlay
        );
    }
}
