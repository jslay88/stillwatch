//! Window size and tray refresh. Split out of `app` so that file stays small.

use iced::window;
use iced::{Size, Task};

use super::{App, AppMessage};
use crate::shell::Pane;
use crate::tray;

pub(super) fn window_settings(pane: Pane) -> window::Settings {
    let size = match pane {
        Pane::Settings => Size::new(840.0, 560.0),
        Pane::Prompt => Size::new(420.0, 240.0),
    };
    window::Settings {
        size,
        ..window::Settings::default()
    }
}

pub(super) fn refresh_tray(app: &mut App) -> Option<Task<AppMessage>> {
    let model = tray::tray_model(&app.shell);
    if app.shown.as_ref() == Some(&model) {
        return None;
    }
    let handle = app.tray.clone()?;
    app.shown = Some(model.clone());
    Some(Task::perform(
        async move {
            handle.update(|tray| tray.set_model(model)).await;
        },
        |()| AppMessage::Nop,
    ))
}
