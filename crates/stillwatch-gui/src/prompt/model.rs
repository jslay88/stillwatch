//! Prompt dialog state. Ticks and answers are injected, so tests don't sleep
//! or open a window.

use stillwatch_core::config::PromptConfig;
use stillwatch_core::state::State;
use stillwatch_ipc::prompt::PromptAnswerKind;

use super::text::parse_custom;

/// What the dialog should do after one input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Stay open.
    Stay,
    /// Send `PromptAnswer(kind, minutes)` and then exit.
    Answer(PromptAnswerKind, u32),
    /// The prompt was resolved elsewhere. Exit without another answer.
    Close,
}

/// One thing that happened to the dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    /// One second passed.
    Tick,
    /// A preset button, in minutes.
    Snooze(u32),
    /// "Blank now".
    BlankNow,
    /// "Cancel" (I'm here).
    Cancel,
    /// The window was closed without a button.
    Dismiss,
    /// Enter: the first preset.
    Enter,
    /// Escape: cancel.
    Escape,
    /// Show or hide the custom duration field.
    ToggleCustom,
    /// The custom field changed.
    CustomText(String),
    /// Submit the custom field.
    SubmitCustom,
    /// `StateChanged`.
    State(State),
    /// A status snapshot, reduced to what the dialog shows.
    Noted(Note),
    /// A bus problem, shown under the buttons.
    Notice(String),
}

/// The part of `Status()` the dialog renders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    /// Daemon state.
    pub state: State,
    /// Why the prompt appeared.
    pub summary: String,
    /// Countdown to apply, when `--remaining` was not set and the dialog
    /// has not ticked yet.
    pub remaining_secs: Option<u64>,
}

/// Whether the custom duration field is available.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CustomField {
    /// `allow_custom` is off.
    Unavailable,
    /// Hidden until "Custom..." is pressed.
    Closed,
    /// The text field is showing.
    Open {
        /// What the user has typed.
        text: String,
        /// Why the last submit was rejected.
        error: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Waiting { ticked: bool },
    Finished,
}

/// The prompt dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dialog {
    pub(crate) presets: Vec<u32>,
    pub(crate) remaining_secs: u64,
    pub(crate) summary: String,
    pub(crate) custom: CustomField,
    pub(crate) notice: Option<String>,
    custom_min: u32,
    custom_max: u32,
    phase: Phase,
}

impl Dialog {
    /// A dialog from `[prompt]`, counting down from `remaining_secs`.
    ///
    /// `custom` opens the custom field immediately (the notification's
    /// "Custom..." action). It does nothing when custom durations are off.
    #[must_use]
    pub fn new(config: &PromptConfig, remaining_secs: u64, custom: bool) -> Self {
        let custom_field = if config.allow_custom {
            if custom {
                CustomField::Open {
                    text: String::new(),
                    error: None,
                }
            } else {
                CustomField::Closed
            }
        } else {
            CustomField::Unavailable
        };
        Self {
            presets: config.snooze_presets_minutes.clone(),
            remaining_secs,
            summary: super::text::STATIC_FALLBACK.to_owned(),
            custom: custom_field,
            notice: None,
            custom_min: config.custom_min_minutes,
            custom_max: config.custom_max_minutes,
            phase: Phase::Waiting { ticked: false },
        }
    }

    /// Seconds left on the countdown.
    #[must_use]
    pub const fn remaining_secs(&self) -> u64 {
        self.remaining_secs
    }

    pub(crate) fn summary(&self) -> &str {
        &self.summary
    }

    pub(crate) fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    /// The custom field's text and error, when it is open.
    pub(crate) fn custom_open(&self) -> Option<(&str, Option<&str>)> {
        match &self.custom {
            CustomField::Open { text, error } => Some((text.as_str(), error.as_deref())),
            CustomField::Closed | CustomField::Unavailable => None,
        }
    }

    pub(crate) const fn custom_available(&self) -> bool {
        !matches!(self.custom, CustomField::Unavailable)
    }
}

/// Applies `input` to `dialog`.
///
/// A finished dialog ignores everything after the first answer or close, so
/// a tick that lands while the answer is in flight cannot also time out.
#[must_use]
pub fn update(dialog: &mut Dialog, input: Input) -> Step {
    if dialog.phase == Phase::Finished {
        return Step::Stay;
    }
    match input {
        Input::Tick => tick(dialog),
        Input::Snooze(minutes) => finish(dialog, Step::Answer(PromptAnswerKind::Snooze, minutes)),
        Input::BlankNow => finish(dialog, Step::Answer(PromptAnswerKind::Timeout, 0)),
        Input::Cancel | Input::Escape => finish(dialog, Step::Answer(PromptAnswerKind::Cancel, 0)),
        Input::Dismiss => finish(dialog, Step::Answer(PromptAnswerKind::Dismissed, 0)),
        Input::Enter => enter(dialog),
        Input::ToggleCustom => {
            toggle_custom(dialog);
            Step::Stay
        }
        Input::CustomText(text) => {
            set_custom_text(dialog, text);
            Step::Stay
        }
        Input::SubmitCustom => submit_custom(dialog),
        Input::State(state) => on_state(dialog, state),
        Input::Noted(note) => note_status(dialog, note),
        Input::Notice(text) => {
            dialog.notice = Some(text);
            Step::Stay
        }
    }
}

fn tick(dialog: &mut Dialog) -> Step {
    dialog.phase = Phase::Waiting { ticked: true };
    if dialog.remaining_secs == 0 {
        return finish(dialog, Step::Answer(PromptAnswerKind::Timeout, 0));
    }
    dialog.remaining_secs -= 1;
    if dialog.remaining_secs == 0 {
        finish(dialog, Step::Answer(PromptAnswerKind::Timeout, 0))
    } else {
        Step::Stay
    }
}

fn enter(dialog: &mut Dialog) -> Step {
    match dialog.presets.first().copied() {
        Some(minutes) => finish(dialog, Step::Answer(PromptAnswerKind::Snooze, minutes)),
        None => Step::Stay,
    }
}

fn on_state(dialog: &mut Dialog, state: State) -> Step {
    if state == State::Prompting {
        Step::Stay
    } else {
        finish(dialog, Step::Close)
    }
}

fn note_status(dialog: &mut Dialog, note: Note) -> Step {
    dialog.summary = note.summary;
    if matches!(dialog.phase, Phase::Waiting { ticked: false })
        && let Some(secs) = note.remaining_secs
    {
        dialog.remaining_secs = secs;
    }
    on_state(dialog, note.state)
}

fn toggle_custom(dialog: &mut Dialog) {
    dialog.custom = match &dialog.custom {
        CustomField::Unavailable => CustomField::Unavailable,
        CustomField::Closed => CustomField::Open {
            text: String::new(),
            error: None,
        },
        CustomField::Open { .. } => CustomField::Closed,
    };
}

fn set_custom_text(dialog: &mut Dialog, text: String) {
    if let CustomField::Open { text: slot, error } = &mut dialog.custom {
        *slot = text;
        *error = None;
    }
}

fn submit_custom(dialog: &mut Dialog) -> Step {
    let Some(text) = dialog.custom_open().map(|(text, _)| text.to_owned()) else {
        return Step::Stay;
    };
    match parse_custom(&text, dialog.custom_min, dialog.custom_max) {
        Ok(minutes) => finish(dialog, Step::Answer(PromptAnswerKind::Snooze, minutes)),
        Err(message) => {
            if let CustomField::Open { error, .. } = &mut dialog.custom {
                *error = Some(message);
            }
            Step::Stay
        }
    }
}

fn finish(dialog: &mut Dialog, step: Step) -> Step {
    dialog.phase = Phase::Finished;
    step
}

#[cfg(test)]
mod tests;
