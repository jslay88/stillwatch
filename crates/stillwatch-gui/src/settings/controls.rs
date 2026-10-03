//! One widget per schema control. The values live on [`Editor`](super::editor::Editor).

use iced::widget::{button, column, pick_list, row, slider, text, text_input, toggler};
use iced::{Element, Fill, Length};
use stillwatch_core::config::Bounds;
use stillwatch_core::schema::{Control, Setting};

use crate::edit_msg::{FieldChange, RegionPart, SettingsMsg};

use super::values::{self, FieldError, FieldValue, RegionInput};

/// The control for `setting`, plus its help, allowed values, and errors.
#[must_use]
pub fn widget<'a>(
    setting: &'a Setting,
    field: &'a FieldValue,
    draft: &'a str,
    issues: &[FieldError],
) -> Element<'a, SettingsMsg> {
    let mut body = column![text(setting.label).size(16), control(setting, field, draft)].spacing(4);
    body = body.push(text(setting.help).size(12));
    if let Some(allowed) = setting.control.allowed() {
        body = body.push(text(allowed_line(&setting.control, &allowed)).size(12));
    }
    if setting.resets_detection {
        body = body.push(text("resets detection").size(12));
    }
    for message in values::field_messages(issues, setting.key) {
        body = body.push(text(message).size(12).style(text::danger));
    }
    body.into()
}

fn allowed_line(control: &Control, allowed: &str) -> String {
    match control {
        Control::Duration { .. } | Control::IntList { unit: Some(_), .. } => {
            format!("Allowed: {}, {allowed}", control.type_name())
        }
        _ => format!("Allowed: {allowed}"),
    }
}

fn control<'a>(
    setting: &'a Setting,
    field: &'a FieldValue,
    draft: &'a str,
) -> Element<'a, SettingsMsg> {
    let key = setting.key;
    match (&setting.control, field) {
        (Control::ReadOnly, FieldValue::Text(value)) => text(value).into(),
        (Control::Toggle, FieldValue::Bool(on)) => {
            let key = key.to_owned();
            toggler(*on)
                .on_toggle(move |value| {
                    SettingsMsg::Edit(FieldChange::Bool {
                        key: key.clone(),
                        value,
                    })
                })
                .into()
        }
        (Control::Enum { choices }, FieldValue::Text(value)) => enum_select(key, choices, value),
        (
            Control::Int { bounds, .. }
            | Control::Percent { bounds }
            | Control::Duration { bounds, .. },
            FieldValue::Text(value),
        ) => number(key, value, *bounds, step_of(&setting.control)),
        (Control::Text | Control::Command, FieldValue::Text(value)) => text_row(key, value, ""),
        (
            Control::StringList
            | Control::OutputPicker
            | Control::GamepadPicker
            | Control::PlayerPicker
            | Control::IntList { .. },
            FieldValue::List(items),
        ) => list_editor(key, items, draft),
        (Control::GridSize { .. }, FieldValue::Grid { cols, rows }) => grid(key, cols, rows),
        (Control::RegionEditor, FieldValue::Regions(regions)) => region_editor(key, regions),
        _ => text("This control doesn't match the saved value.").into(),
    }
}

fn step_of(control: &Control) -> u32 {
    match control {
        Control::Int { step, .. } => *step,
        _ => 1,
    }
}

fn enum_select<'a>(
    key: &'a str,
    choices: &'a [stillwatch_core::schema::Choice],
    value: &'a str,
) -> Element<'a, SettingsMsg> {
    let options: Vec<String> = choices
        .iter()
        .map(|choice| choice.value.to_owned())
        .collect();
    let selected = options
        .iter()
        .find(|option| option.as_str() == value)
        .cloned();
    let key = key.to_owned();
    pick_list(options, selected, move |value| {
        SettingsMsg::Edit(FieldChange::Text {
            key: key.clone(),
            value,
        })
    })
    .into()
}

fn number<'a>(key: &'a str, value: &'a str, bounds: Bounds, step: u32) -> Element<'a, SettingsMsg> {
    let input = text_row(key, value, "0");
    let Some(max) = u16::try_from(bounds.max)
        .ok()
        .filter(|_| bounds.upper().is_some())
    else {
        return input;
    };
    let Ok(min) = u16::try_from(bounds.min) else {
        return input;
    };
    let current = value.parse::<u16>().unwrap_or(min).clamp(min, max);
    let key_owned = key.to_owned();
    let stepped = u16::try_from(step).unwrap_or(1).max(1);
    let slider = slider(
        f64::from(min)..=f64::from(max),
        f64::from(current),
        move |next| {
            SettingsMsg::Edit(FieldChange::Text {
                key: key_owned.clone(),
                value: whole_text(next),
            })
        },
    )
    .step(f64::from(stepped));
    row![slider, input].spacing(8).into()
}

fn whole_text(value: f64) -> String {
    let capped = value.clamp(0.0, f64::from(u16::MAX));
    format!("{capped:.0}")
}

fn text_row<'a>(key: &'a str, value: &'a str, placeholder: &'a str) -> Element<'a, SettingsMsg> {
    let key = key.to_owned();
    text_input(placeholder, value)
        .on_input(move |value| {
            SettingsMsg::Edit(FieldChange::Text {
                key: key.clone(),
                value,
            })
        })
        .width(Length::Fixed(120.0))
        .into()
}

fn list_editor<'a>(key: &'a str, items: &'a [String], draft: &'a str) -> Element<'a, SettingsMsg> {
    let mut body = column![].spacing(4);
    for (index, item) in items.iter().enumerate() {
        let edit_key = key.to_owned();
        let remove_key = key.to_owned();
        body = body.push(
            row![
                text_input("", item)
                    .on_input(move |value| {
                        SettingsMsg::Edit(FieldChange::ListItem {
                            key: edit_key.clone(),
                            index,
                            value,
                        })
                    })
                    .width(Fill),
                button("Remove").on_press(SettingsMsg::Edit(FieldChange::ListRemove {
                    key: remove_key,
                    index,
                })),
            ]
            .spacing(8),
        );
    }
    let draft_key = key.to_owned();
    let push_key = key.to_owned();
    body = body.push(
        row![
            text_input("Add", draft)
                .on_input(move |value| {
                    SettingsMsg::Edit(FieldChange::ListDraft {
                        key: draft_key.clone(),
                        value,
                    })
                })
                .width(Fill),
            button("Add").on_press(SettingsMsg::Edit(FieldChange::ListPush { key: push_key })),
        ]
        .spacing(8),
    );
    body.into()
}

fn grid<'a>(key: &'a str, cols: &'a str, rows: &'a str) -> Element<'a, SettingsMsg> {
    let cols_key = key.to_owned();
    let cols_now = cols.to_owned();
    let rows_key = key.to_owned();
    let rows_now = rows.to_owned();
    row![
        text("Columns"),
        text_input("cols", cols)
            .on_input(move |value| {
                SettingsMsg::Edit(FieldChange::Grid {
                    key: cols_key.clone(),
                    cols: value,
                    rows: rows_now.clone(),
                })
            })
            .width(Length::Fixed(80.0)),
        text("Rows"),
        text_input("rows", rows)
            .on_input(move |value| {
                SettingsMsg::Edit(FieldChange::Grid {
                    key: rows_key.clone(),
                    cols: cols_now.clone(),
                    rows: value,
                })
            })
            .width(Length::Fixed(80.0)),
    ]
    .spacing(8)
    .into()
}

fn region_editor<'a>(key: &'a str, regions: &'a [RegionInput]) -> Element<'a, SettingsMsg> {
    let mut body = column![].spacing(6);
    for (index, region) in regions.iter().enumerate() {
        body = body.push(region_row(key, index, region));
    }
    body = body.push(
        button("Add region").on_press(SettingsMsg::Edit(FieldChange::RegionPush {
            key: key.to_owned(),
        })),
    );
    body.into()
}

fn region_row<'a>(key: &'a str, index: usize, region: &'a RegionInput) -> Element<'a, SettingsMsg> {
    let remove_key = key.to_owned();
    row![
        region_input(
            key,
            index,
            RegionPart::Output,
            "output",
            &region.output,
            140.0
        ),
        region_input(key, index, RegionPart::X, "x", &region.x, 64.0),
        region_input(key, index, RegionPart::Y, "y", &region.y, 64.0),
        region_input(key, index, RegionPart::W, "w", &region.w, 64.0),
        region_input(key, index, RegionPart::H, "h", &region.h, 64.0),
        button("Remove").on_press(SettingsMsg::Edit(FieldChange::RegionRemove {
            key: remove_key,
            index,
        })),
    ]
    .spacing(6)
    .into()
}

fn region_input<'a>(
    key: &'a str,
    index: usize,
    part: RegionPart,
    placeholder: &'a str,
    value: &'a str,
    width: f32,
) -> Element<'a, SettingsMsg> {
    let key = key.to_owned();
    text_input(placeholder, value)
        .on_input(move |value| {
            SettingsMsg::Edit(FieldChange::Region {
                key: key.clone(),
                index,
                field: part,
                value,
            })
        })
        .width(Length::Fixed(width))
        .into()
}
