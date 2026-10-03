//! [`update`] applies a [`Message`] to a [`Shell`] and returns the daemon
//! calls the tray asked for.

use stillwatch_core::state::State;

use crate::settings;
use crate::shell::{
    DaemonCall, DaemonEvent, Link, Message, Shell, Snapshot, TrayAction, Visibility, snooze_seconds,
};

/// Changes `shell` for `message` and returns the daemon calls to make.
///
/// Window open/close and page routing stay on `shell`. Calls are returned
/// even when the daemon is currently down, so the watcher can report the
/// failure instead of the menu pretending the click did something.
#[must_use]
pub fn update(shell: &mut Shell, message: Message) -> Vec<DaemonCall> {
    match message {
        Message::Navigate(page) => shell.page = page,
        Message::OpenSettings => shell.settings = shell.settings.reveal(),
        Message::CloseSettings => shell.settings = Visibility::Closed,
        Message::OpenPrompt => shell.prompt = shell.prompt.reveal(),
        Message::ClosePrompt => shell.prompt = Visibility::Closed,
        Message::Quit => shell.quit = true,
        Message::Settings(message) => return settings_msg(shell, message),
        Message::Tray(action) => return tray(shell, action),
        Message::BecamePrimary { first, mode } => shell.apply_launch(mode, first),
        Message::Daemon(event) => apply_daemon(shell, event),
    }
    Vec::new()
}

fn tray(shell: &mut Shell, action: TrayAction) -> Vec<DaemonCall> {
    let call = match action {
        TrayAction::Snooze { minutes } => Some(DaemonCall::Snooze {
            seconds: snooze_seconds(minutes),
        }),
        TrayAction::CancelSnooze => Some(DaemonCall::CancelSnooze),
        TrayAction::Pause => Some(DaemonCall::Pause),
        TrayAction::Resume => Some(DaemonCall::Resume),
        TrayAction::OpenSettings => {
            shell.settings = shell.settings.reveal();
            None
        }
        TrayAction::Quit => {
            shell.quit = true;
            None
        }
    };
    call.into_iter().collect()
}

fn apply_daemon(shell: &mut Shell, event: DaemonEvent) {
    match event {
        DaemonEvent::Down => {
            shell.link = Link::Down;
            shell.notice = None;
        }
        DaemonEvent::Snapshot(snapshot) => {
            shell.config_ok = Some(snapshot.config_errors.is_empty());
            shell.config_errors.clone_from(&snapshot.config_errors);
            shell.link = Link::Up(snapshot);
            shell.notice = None;
        }
        DaemonEvent::State(state) => apply_state(shell, state),
        DaemonEvent::Config { ok, errors } => {
            shell.config_ok = Some(ok);
            shell.config_errors = errors;
            if let Some(path) = shell.config_path.clone() {
                let dirty = shell.editor.is_dirty();
                shell.editor.on_disk_changed(&path);
                if !dirty && let Some(presets) = shell.editor.presets() {
                    shell.presets_minutes = presets;
                }
            }
        }
        DaemonEvent::Presets(presets) => shell.presets_minutes = presets,
        DaemonEvent::CallFailed(message) => shell.notice = Some(message),
    }
}

fn settings_msg(shell: &mut Shell, message: crate::edit_msg::SettingsMsg) -> Vec<DaemonCall> {
    let outcome: settings::Outcome =
        settings::handle(&mut shell.editor, shell.config_path.as_deref(), message);
    if let Some(presets) = outcome.presets {
        shell.presets_minutes = presets;
    }
    if outcome.reload && matches!(shell.link, Link::Up(_)) {
        vec![DaemonCall::Reload]
    } else {
        Vec::new()
    }
}

fn apply_state(shell: &mut Shell, state: State) {
    let previous = match &shell.link {
        Link::Up(snapshot) => Some(snapshot.clone()),
        Link::Down => None,
    };
    let remaining = previous
        .as_ref()
        .filter(|_| state == State::Snoozed)
        .and_then(|snapshot| snapshot.snooze_remaining_seconds);
    let config_errors = previous.map_or_else(Vec::new, |snapshot| snapshot.config_errors);
    shell.link = Link::Up(Snapshot {
        state,
        snooze_remaining_seconds: remaining,
        config_errors,
    });
}

#[cfg(test)]
mod tests;
