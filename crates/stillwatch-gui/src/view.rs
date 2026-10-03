//! The settings window and the prompt placeholder. Widgets only: state
//! changes go through [`crate::model::update`].

use iced::widget::{button, column, container, row, text};
use iced::{Element, Fill};

use crate::page::Page;
use crate::shell::{Message, Shell};
use crate::tray::status_text;

/// The settings window: navigation plus the current page's placeholder.
#[must_use]
pub fn shell(shell: &Shell) -> Element<'_, Message> {
    let mut content = column![
        text(status_text(&shell.link)),
        nav(shell.page),
        text(shell.page.label()).size(24),
        text(shell.page.placeholder()),
    ]
    .spacing(12);
    if let Some(notice) = &shell.notice {
        content = content.push(text(notice));
    }
    if shell.config_ok == Some(false) {
        content = content.push(text(config_line(&shell.config_errors)));
    }
    content = content.push(button("Quit").on_press(Message::Quit));
    container(content)
        .padding(16)
        .width(Fill)
        .height(Fill)
        .into()
}

/// The prompt window. The dialog itself is a later change.
#[must_use]
pub fn prompt() -> Element<'static, Message> {
    column![
        text("Prompt").size(24),
        text("The prompt dialog is not built yet."),
        button("Close").on_press(Message::ClosePrompt),
    ]
    .spacing(12)
    .padding(16)
    .into()
}

fn nav(current: Page) -> Element<'static, Message> {
    let mut buttons = row![].spacing(8);
    for page in Page::ALL {
        let mut item = button(text(page.label())).on_press(Message::Navigate(page));
        if page == current {
            item = item.style(button::primary);
        }
        buttons = buttons.push(item);
    }
    buttons.into()
}

fn config_line(errors: &[String]) -> String {
    match errors.first() {
        Some(error) => format!("Config reload failed: {error}"),
        None => "Config reload failed.".to_owned(),
    }
}
