//! Checkbox rows for the output, gamepad, and player controls.
//!
//! The add row is the same free-text path the other lists use, so a name can
//! be typed when the daemon is down or the device isn't connected.

use iced::widget::{column, row, text, toggler};
use iced::{Element, Fill};
use stillwatch_core::schema::Control;

use crate::edit_msg::{FieldChange, SettingsMsg};

use super::catalog::Catalog;
use super::controls::draft_row;
use super::pickers::{self, PickerRow, WATCHES_ALL};

/// The picker for `control`, plus a free-text add row.
#[must_use]
pub fn control<'a>(
    key: &'a str,
    kind: &Control,
    items: &'a [String],
    draft: &'a str,
    devices: &'a Catalog,
) -> Element<'a, SettingsMsg> {
    let rows = match kind {
        Control::OutputPicker => pickers::output_rows(items, &devices.outputs),
        Control::GamepadPicker => pickers::gamepad_rows(items, &devices.gamepads),
        Control::PlayerPicker => pickers::player_rows(items, &devices.players),
        _ => Vec::new(),
    };
    let mut body = column![].spacing(4);
    if matches!(kind, Control::OutputPicker) && items.is_empty() {
        body = body.push(text(WATCHES_ALL).size(12));
    }
    for entry in rows {
        body = body.push(entry_row(key, kind, items, devices, &entry));
    }
    body.push(draft_row(key, draft)).into()
}

fn entry_row<'a>(
    key: &'a str,
    kind: &Control,
    items: &'a [String],
    devices: &'a Catalog,
    entry: &PickerRow,
) -> Element<'a, SettingsMsg> {
    let key_owned = key.to_owned();
    let value = entry.value.clone();
    let current = items.to_vec();
    let players = devices.players.clone();
    let kind = *kind;
    let connected = entry.connected;
    let toggle = toggler(entry.selected).on_toggle(move |on| {
        let next = if connected {
            match kind {
                Control::GamepadPicker => pickers::set_gamepad(&current, &value, on),
                Control::PlayerPicker => pickers::set_player(&current, &value, &players, on),
                _ => pickers::set_exact(&current, &value, on),
            }
        } else {
            pickers::set_exact(&current, &value, on)
        };
        SettingsMsg::Edit(FieldChange::ListSet {
            key: key_owned.clone(),
            items: next,
        })
    });
    let mut line = row![toggle, text(entry.label.clone())].spacing(8);
    if let Some(activity) = entry.activity_label() {
        line = line.push(text(activity).size(12));
    }
    if let Some(absence) = entry.absence() {
        line = line.push(text(absence).size(12));
    }
    line.width(Fill).into()
}
