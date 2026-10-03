//! The iced window around [`super::model`].

use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};

use iced::Element;
use iced::keyboard::Key;
use iced::keyboard::key::Named;
use iced::window::{self, Level, Position};
use iced::{Size, Subscription, Task, Theme};

use super::model::{Dialog, Input, Note, Step, update};
use super::text::{remaining_secs, static_summary};
use super::view;
use super::watch::{self, Wake};
use super::{Launch, answer_at};
use crate::error::Error;
use stillwatch_core::config::PromptConfig;
use stillwatch_ipc::config_file;
use stillwatch_ipc::prompt::PromptAnswerKind;
use stillwatch_ipc::status::StatusPayload;

/// Runtime state for one prompt window.
pub(crate) struct App {
    dialog: Dialog,
    exit_code: Arc<AtomicI32>,
    address: Option<String>,
    countdown: u32,
    /// `--remaining` was set, so status must not replace the countdown.
    remaining_fixed: bool,
    /// Exit code when the in-flight answer can't be sent.
    fail_code: i32,
}

pub(crate) enum Message {
    Ui(Input),
    Wake(Wake),
    Sent(Result<(), String>),
}

/// Builds the iced application and runs it.
///
/// # Errors
///
/// Returns the iced error when the runtime can't start.
pub(crate) fn run(launch: Launch, exit_code: Arc<AtomicI32>) -> iced::Result {
    iced::application(
        move || boot(launch.clone(), Arc::clone(&exit_code)),
        update_app,
        view_app,
    )
    .title(title)
    .subscription(subscription)
    .theme(theme)
    .window(window_settings())
    .run()
}

/// Centered, always on top, and kept open when the title bar is closed so
/// the close can be sent as `Dismissed`.
///
/// Always-on-top is winit's X11 `_NET_WM_STATE_ABOVE`. The GUI stays on
/// iced's x11 backend; the Wayland backend is not enabled.
#[must_use]
pub(crate) fn window_settings() -> window::Settings {
    window::Settings {
        size: Size::new(440.0, 360.0),
        position: Position::Centered,
        level: Level::AlwaysOnTop,
        exit_on_close_request: false,
        ..window::Settings::default()
    }
}

fn boot(launch: Launch, exit_code: Arc<AtomicI32>) -> (App, Task<Message>) {
    let prompt = load_prompt();
    let countdown = prompt.countdown_seconds;
    let remaining = launch.remaining.unwrap_or(u64::from(countdown));
    let app = App {
        dialog: Dialog::new(&prompt, remaining, launch.custom),
        exit_code,
        address: launch.address,
        countdown,
        remaining_fixed: launch.remaining.is_some(),
        fail_code: 2,
    };
    let task = if remaining == 0 {
        Task::done(Message::Ui(Input::Tick))
    } else {
        Task::none()
    };
    (app, task)
}

fn load_prompt() -> PromptConfig {
    match config_file::load_default() {
        Ok(loaded) => loaded.config.prompt,
        Err(err) => {
            tracing::warn!(%err, "using default prompt settings");
            PromptConfig::default()
        }
    }
}

fn theme(_app: &App) -> Theme {
    Theme::Dark
}

fn title(_app: &App) -> String {
    "Stillwatch".to_owned()
}

fn view_app(app: &App) -> Element<'_, Message> {
    view::view(&app.dialog).map(Message::Ui)
}

fn subscription(app: &App) -> Subscription<Message> {
    Subscription::batch([
        iced::event::listen_with(|event, _status, _window| match event {
            iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { key, repeat, .. })
                if !repeat =>
            {
                match key {
                    Key::Named(Named::Enter) => Some(Message::Ui(Input::Enter)),
                    Key::Named(Named::Escape) => Some(Message::Ui(Input::Escape)),
                    _ => None,
                }
            }
            _ => None,
        }),
        iced::time::every(std::time::Duration::from_secs(1)).map(|_| Message::Ui(Input::Tick)),
        window::close_requests().map(|_| Message::Ui(Input::Dismiss)),
        watch::subscription(app.address.as_deref()).map(Message::Wake),
    ])
}

fn update_app(app: &mut App, message: Message) -> Task<Message> {
    match message {
        Message::Ui(input) => apply(app, input),
        Message::Wake(Wake::Status(status)) => apply(app, Input::Noted(note_from(app, &status))),
        Message::Wake(Wake::State(state)) => apply(app, Input::State(state)),
        Message::Wake(Wake::Notice(text)) => apply(app, Input::Notice(text)),
        Message::Sent(Ok(())) => {
            app.exit_code.store(0, Ordering::SeqCst);
            iced::exit()
        }
        Message::Sent(Err(err)) => {
            tracing::warn!(%err, "prompt answer was not delivered");
            app.exit_code.store(app.fail_code, Ordering::SeqCst);
            iced::exit()
        }
    }
}

fn note_from(app: &App, status: &StatusPayload) -> Note {
    Note {
        state: status.state,
        summary: static_summary(status),
        remaining_secs: (!app.remaining_fixed).then(|| remaining_secs(status, app.countdown)),
    }
}

fn apply(app: &mut App, input: Input) -> Task<Message> {
    match update(&mut app.dialog, input) {
        Step::Stay => Task::none(),
        Step::Close => {
            app.exit_code.store(0, Ordering::SeqCst);
            iced::exit()
        }
        Step::Answer(kind, minutes) => {
            app.fail_code = if kind == PromptAnswerKind::Dismissed {
                1
            } else {
                2
            };
            let address = app.address.clone();
            Task::perform(
                async move {
                    answer_at(address.as_deref(), kind, minutes)
                        .await
                        .map_err(|err: Error| err.to_string())
                },
                Message::Sent,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_is_centered_and_always_on_top() {
        let settings = window_settings();
        assert_eq!(settings.level, Level::AlwaysOnTop);
        assert!(matches!(settings.position, Position::Centered));
        assert!(!settings.exit_on_close_request);
        assert_eq!(settings.size, Size::new(440.0, 360.0));
    }
}
