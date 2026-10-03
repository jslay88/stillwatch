//! Service page widgets.

use iced::widget::{button, column, row, scrollable, text, toggler};
use iced::{Element, Fill};

use super::unit::{UnitView, unit_sentence};
use super::{SvcMsg, panel_lines};
use crate::page::Page;
use crate::shell::{Message, Shell};

/// The service page: unit controls, tray autostart, and panel care.
#[must_use]
pub fn page(shell: &Shell) -> Element<'_, Message> {
    let unit = &shell.service.unit;
    let mut body = column![
        text(Page::Service.label()).size(24),
        text(Page::Service.placeholder()),
        text(unit_sentence(unit)),
        actions(unit),
        autostart_row(shell.service.autostart),
        text("Panel care").size(16),
    ]
    .spacing(8);
    for line in panel_lines(shell.panel_care.as_ref()) {
        body = body.push(text(line));
    }
    if let Some(block) = journal_block(unit) {
        body = body.push(block);
    }
    scrollable(body).height(Fill).into()
}

fn actions(unit: &UnitView) -> Element<'_, Message> {
    let UnitView::Ready { enabled, .. } = unit else {
        return text("").into();
    };
    let enable = if *enabled {
        action("Disable at login", SvcMsg::Disable)
    } else {
        action("Enable at login", SvcMsg::Enable)
    };
    row![
        action("Start", SvcMsg::Start),
        action("Stop", SvcMsg::Stop),
        action("Restart", SvcMsg::Restart),
        enable,
    ]
    .spacing(8)
    .into()
}

fn action(label: &str, message: SvcMsg) -> Element<'_, Message> {
    button(text(label))
        .on_press(Message::Service(message))
        .into()
}

fn autostart_row(enabled: Option<bool>) -> Element<'static, Message> {
    let Some(enabled) = enabled else {
        return text("Checking tray autostart.").into();
    };
    row![
        text("Start the tray at login"),
        toggler(enabled).on_toggle(|value| Message::Service(SvcMsg::Autostart(value))),
    ]
    .spacing(8)
    .into()
}

fn journal_block(unit: &UnitView) -> Option<Element<'_, Message>> {
    let UnitView::Ready { run, .. } = unit else {
        return None;
    };
    if !run.is_failed() {
        return None;
    }
    let mut block = column![text("Recent journal (last 50 lines).").size(16)].spacing(4);
    let lines = unit.journal();
    if lines.is_empty() {
        block = block.push(text("No recent journal lines."));
    }
    for line in lines {
        block = block.push(text(line).size(12));
    }
    block = block.push(button(text("Open full log")).on_press(Message::Service(SvcMsg::OpenLog)));
    Some(block.into())
}
