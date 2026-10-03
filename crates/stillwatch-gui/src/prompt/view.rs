//! Widgets for the prompt dialog. State changes go through [`update`](super::update).

use iced::widget::{button, column, row, text, text_input};
use iced::{Element, Fill};

use super::model::{Dialog, Input};
use super::text::{countdown_line, preset_label};

/// The prompt window.
#[must_use]
pub(crate) fn view(dialog: &Dialog) -> Element<'_, Input> {
    let mut content = column![
        text(dialog.summary()),
        text(countdown_line(dialog.remaining_secs())).size(28),
        presets(dialog),
    ]
    .spacing(12)
    .padding(16)
    .width(Fill);
    content = match dialog.custom_open() {
        Some((value, error)) => content.push(custom_field(value, error)),
        None if dialog.custom_available() => {
            content.push(button("Custom...").on_press(Input::ToggleCustom))
        }
        None => content,
    };
    content = content.push(
        row![
            button("Blank now").on_press(Input::BlankNow),
            button("Cancel").on_press(Input::Cancel),
        ]
        .spacing(8),
    );
    if let Some(notice) = dialog.notice() {
        content = content.push(text(notice));
    }
    content.into()
}

fn presets(dialog: &Dialog) -> Element<'_, Input> {
    let mut buttons = row![].spacing(8);
    for (index, minutes) in dialog.presets.iter().copied().enumerate() {
        let mut item = button(text(preset_label(minutes))).on_press(Input::Snooze(minutes));
        if index == 0 {
            item = item.style(button::primary);
        }
        buttons = buttons.push(item);
    }
    buttons.into()
}

fn custom_field<'a>(value: &'a str, error: Option<&'a str>) -> Element<'a, Input> {
    let mut field = column![
        text_input("45m", value)
            .on_input(Input::CustomText)
            .width(Fill),
        button("Snooze").on_press(Input::SubmitCustom),
    ]
    .spacing(8);
    if let Some(error) = error {
        field = field.push(text(error));
    }
    field.into()
}
