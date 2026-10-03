//! The settings page. Widgets only; edits go through [`SettingsMsg`](crate::edit_msg::SettingsMsg).

use iced::widget::{button, column, row, scrollable, text};
use iced::{Element, Fill};
use stillwatch_core::schema::{self, Section};

use crate::edit_msg::{RestoreScope, SettingsMsg};

use super::catalog::Catalog;
use super::controls;
use super::editor::{Banner, Editor};
use super::preset_view;
use super::values::{self, FieldError};

/// The schema-driven settings form.
#[must_use]
pub fn page<'a>(
    editor: &'a Editor,
    external: &'a [String],
    devices: &'a Catalog,
) -> Element<'a, SettingsMsg> {
    let (keyed, other) = values::external_issues(external);
    let mut issues = editor.issues();
    issues.extend(keyed);

    let mut form = column![preset_view::block(editor, devices)].spacing(18);
    for section in schema::SECTIONS {
        form = form.push(section_block(editor, section, &issues, devices));
    }

    let mut page = column![].spacing(12).height(Fill);
    if editor.banner() == Some(Banner::DiskChanged) {
        page = page.push(banner());
    }
    if let Some(prompt) = editor.confirm_prompt() {
        page = page.push(confirm(prompt));
    }
    for line in editor
        .load_error()
        .into_iter()
        .chain(editor.save_error())
        .chain(other.iter().map(String::as_str))
    {
        page = page.push(text(line.to_owned()).size(12).style(text::danger));
    }
    for line in values::global_messages(&issues) {
        page = page.push(text(line).size(12).style(text::danger));
    }
    page = page.push(scrollable(form).height(Fill));
    page = page.push(footer(editor));
    page.into()
}

fn section_block<'a>(
    editor: &'a Editor,
    section: &'a Section,
    issues: &[FieldError],
    devices: &'a Catalog,
) -> Element<'a, SettingsMsg> {
    let mut body = column![
        row![
            text(section.title).size(20),
            button("Restore defaults").on_press(SettingsMsg::AskRestore(RestoreScope::Section(
                section.id.to_owned()
            ))),
        ]
        .spacing(12),
        text(section.help).size(12),
    ]
    .spacing(8);
    for setting in section.settings {
        let Some(field) = editor.field(setting.key) else {
            continue;
        };
        body = body.push(controls::widget(
            setting,
            field,
            editor.draft(setting.key),
            issues,
            devices,
        ));
    }
    body.into()
}

fn banner() -> Element<'static, SettingsMsg> {
    column![
        text("The config file changed on disk."),
        row![
            button("Reload from disk").on_press(SettingsMsg::ReloadDisk),
            button("Keep my edits").on_press(SettingsMsg::KeepEdits),
        ]
        .spacing(8),
    ]
    .spacing(8)
    .into()
}

fn confirm(prompt: String) -> Element<'static, SettingsMsg> {
    column![
        text(prompt),
        row![
            button("Restore").on_press(SettingsMsg::ConfirmRestore),
            button("Cancel").on_press(SettingsMsg::CancelRestore),
        ]
        .spacing(8),
    ]
    .spacing(8)
    .into()
}

fn footer(editor: &Editor) -> Element<'static, SettingsMsg> {
    let mut save = button("Save");
    if editor.can_save() {
        save = save.on_press(SettingsMsg::Save);
    }
    row![
        button("Restore all defaults").on_press(SettingsMsg::AskRestore(RestoreScope::All)),
        save,
    ]
    .spacing(8)
    .into()
}
