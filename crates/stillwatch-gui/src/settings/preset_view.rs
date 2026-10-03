//! The display-preset block at the top of the settings page.

use iced::widget::{button, column, row, text, text_input, toggler};
use iced::{Element, Fill};

use crate::edit_msg::{PresetKind, SettingsMsg};

use super::catalog::Catalog;
use super::editor::Editor;
use super::pickers::{self, NOT_CONNECTED};
use super::presets::{self, OLED_TV_BLANK_HOOK, OLED_TV_RESUME_HOOK};

/// Preset buttons, the review diff, and the mixed-output prompt.
#[must_use]
pub fn block<'a>(editor: &'a Editor, devices: &'a Catalog) -> Element<'a, SettingsMsg> {
    let mut body = column![
        text("Display preset").size(20),
        text(
            "A preset writes the settings in the diff and nothing else. \
             Apply saves the file the same way Save does. There is no profile key."
        )
        .size(12),
        buttons(),
    ]
    .spacing(8);

    let Some(draft) = editor.preset() else {
        return body.into();
    };
    if draft.kind == PresetKind::Mixed {
        body = body.push(text("Which outputs are OLED?").size(16));
        body = body.push(mixed(draft, devices));
    }
    if draft.kind == PresetKind::OledTvHooks {
        body = body.push(
            text(format!(
                "Hook examples: `{OLED_TV_BLANK_HOOK}` and `{OLED_TV_RESUME_HOOK}`. \
                 Edit them after applying if the TV uses a different tool."
            ))
            .size(12),
        );
    }

    let diff = editor.preset_changes();
    if diff.is_empty() {
        let note = if draft.kind == PresetKind::Custom {
            "Custom doesn't change any settings."
        } else {
            "These settings already match."
        };
        body = body.push(text(note).size(12));
        return body
            .push(button("Dismiss").on_press(SettingsMsg::CancelPreset))
            .into();
    }

    for change in diff {
        body = body.push(text(format!(
            "{}: {} -> {}",
            change.key, change.before, change.after
        )));
    }
    body.push(
        row![
            button("Apply").on_press(SettingsMsg::ApplyPreset),
            button("Cancel").on_press(SettingsMsg::CancelPreset),
        ]
        .spacing(8),
    )
    .into()
}

fn buttons() -> Element<'static, SettingsMsg> {
    let mut line = row![].spacing(8);
    for kind in PresetKind::ALL {
        line = line.push(button(text(kind.label())).on_press(SettingsMsg::PreviewPreset(kind)));
    }
    line.into()
}

fn mixed<'a>(draft: &'a presets::PresetDraft, devices: &'a Catalog) -> Element<'a, SettingsMsg> {
    let mut body = column![].spacing(4);
    for entry in pickers::output_rows(&draft.outputs, &devices.outputs) {
        let name = entry.value.clone();
        let toggle = toggler(entry.selected).on_toggle(move |on| SettingsMsg::MixedToggle {
            name: name.clone(),
            on,
        });
        let mut line = row![toggle, text(entry.label.clone())].spacing(8);
        if !entry.connected {
            line = line.push(text(NOT_CONNECTED).size(12));
        }
        body = body.push(line);
    }
    body.push(
        row![
            text_input("Add an output", &draft.draft)
                .on_input(SettingsMsg::MixedDraft)
                .width(Fill),
            button("Add").on_press(SettingsMsg::MixedPush),
        ]
        .spacing(8),
    )
    .into()
}
