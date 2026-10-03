//! History page widgets.

use iced::widget::{button, column, pick_list, row, scrollable, text};
use iced::{Element, Fill};

use super::{HistMsg, HistoryPage, HistorySource, KindFilter, TimeRange, now_seconds};
use crate::page::Page;
use crate::shell::{Message, Shell};

/// The history page: range, kind, list, and the selected entry.
#[must_use]
pub fn page(shell: &Shell) -> Element<'_, Message> {
    let history = &shell.history;
    let selected = history.selected;
    let error = history.error.clone();
    let source = history.source;
    let rows = history.shown(now_seconds());

    let mut body = column![
        text(Page::History.label()).size(24),
        text(Page::History.placeholder()),
        filters(history),
        text(source_line(source)).size(12),
    ]
    .spacing(8);

    if let Some(error) = error {
        body = body.push(text(error).style(text::danger));
    }
    if rows.is_empty() {
        body = body.push(text("No entries in this range."));
    }
    for row in &rows {
        let mut item =
            button(text(row.label.clone())).on_press(Message::History(HistMsg::Select(row.at)));
        if selected == Some(row.at) {
            item = item.style(button::primary);
        }
        body = body.push(item);
        if selected == Some(row.at) {
            body = body.push(detail(row.detail.clone()));
        }
    }
    scrollable(body).height(Fill).into()
}

fn filters(history: &HistoryPage) -> Element<'_, Message> {
    let mut ranges = row![].spacing(8);
    for range in TimeRange::ALL {
        let mut item =
            button(text(range.label())).on_press(Message::History(HistMsg::Range(range)));
        if range == history.range {
            item = item.style(button::primary);
        }
        ranges = ranges.push(item);
    }
    let kind = history.kind;
    row![
        ranges,
        pick_list(KindFilter::choices(), Some(kind), |choice| {
            Message::History(HistMsg::Kind(choice))
        }),
    ]
    .spacing(12)
    .into()
}

fn detail(lines: Vec<String>) -> Element<'static, Message> {
    let mut block = column![].spacing(2);
    for line in lines {
        block = block.push(text(line).size(14));
    }
    block.into()
}

fn source_line(source: Option<HistorySource>) -> &'static str {
    match source {
        Some(HistorySource::Daemon) => "Loaded from stillwatchd.",
        Some(HistorySource::File) => "Loaded from the history file. The daemon is down.",
        None => "History has not loaded yet.",
    }
}
