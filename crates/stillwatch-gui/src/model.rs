//! [`update`] applies a [`Message`] to a [`Shell`] and returns the daemon
//! calls the tray asked for.

use stillwatch_core::state::State;

use crate::calibration::{self, CalMsg};
use crate::history::{self, HistMsg};
use crate::page::Page;
use crate::service::{self, SvcMsg};
use crate::settings::{self, Catalog};
use crate::shell::{
    DaemonCall, DaemonEvent, Link, Message, Shell, Snapshot, TrayAction, Visibility, snooze_seconds,
};

/// Changes `shell` for `message` and returns the daemon calls to make.
///
/// Window open/close and page routing stay on `shell`. Calls are returned
/// even when the daemon is currently down, so the watcher can report the
/// failure instead of the menu pretending the click did something.
/// The calibration page adds `StartProbe` while it is showing and `StopProbe`
/// when it is left or the window closes.
#[must_use]
pub fn update(shell: &mut Shell, message: Message) -> Vec<DaemonCall> {
    let mut calls = apply(shell, message);
    if let Some(call) = probe_call(shell) {
        calls.push(call);
    }
    calls
}

fn probe_call(shell: &mut Shell) -> Option<DaemonCall> {
    let visible = shell.settings.is_open() && shell.page == Page::Calibration;
    let up = matches!(shell.link, Link::Up(_));
    let seconds = shell
        .editor
        .number("stale.check_interval_seconds")
        .unwrap_or(60);
    let interval = calibration::interval_ms(shell.calibration.pace, seconds);
    calibration::next_call(&mut shell.calibration, visible, up, interval)
}

fn apply(shell: &mut Shell, message: Message) -> Vec<DaemonCall> {
    match message {
        Message::Navigate(page) => {
            shell.page = page;
            return page_calls(shell);
        }
        Message::OpenSettings => {
            shell.settings = shell.settings.reveal();
            let mut calls = refresh_devices(shell);
            calls.extend(page_calls(shell));
            return calls;
        }
        Message::PollPage => return page_calls(shell),
        Message::CloseSettings => shell.settings = Visibility::Closed,
        Message::OpenPrompt => shell.prompt = shell.prompt.reveal(),
        Message::ClosePrompt => shell.prompt = Visibility::Closed,
        Message::Quit => shell.quit = true,
        Message::Settings(message) => return settings_msg(shell, message),
        Message::Calibration(message) => return calibration_msg(shell, message),
        Message::History(message) => return history::apply(&mut shell.history, message),
        Message::Service(message) => return service::apply(&mut shell.service, message),
        Message::Tray(action) => return tray(shell, action),
        Message::BecamePrimary { first, mode } => {
            shell.apply_launch(mode, first);
            let mut calls = refresh_devices(shell);
            calls.extend(page_calls(shell));
            return calls;
        }
        Message::Daemon(event) => return apply_daemon(shell, event),
    }
    Vec::new()
}

/// History and the user unit are polled only while their page is open.
#[must_use]
pub(crate) fn page_poll(shell: &Shell) -> bool {
    shell.settings.is_open() && matches!(shell.page, Page::History | Page::Service)
}

fn page_calls(shell: &Shell) -> Vec<DaemonCall> {
    if !shell.settings.is_open() {
        return Vec::new();
    }
    match shell.page {
        Page::History => vec![history_call(shell)],
        Page::Service => vec![DaemonCall::RefreshUnit, DaemonCall::ReadAutostart],
        Page::Settings | Page::Calibration => Vec::new(),
    }
}

fn history_call(shell: &Shell) -> DaemonCall {
    DaemonCall::LoadHistory {
        since_seconds: shell.history.since_seconds(),
    }
}

fn refresh_devices(shell: &Shell) -> Vec<DaemonCall> {
    if shell.settings.is_open() && matches!(shell.link, Link::Up(_)) {
        vec![DaemonCall::RefreshDevices]
    } else {
        Vec::new()
    }
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
            return refresh_devices(shell);
        }
        TrayAction::Quit => {
            shell.quit = true;
            None
        }
    };
    call.into_iter().collect()
}

fn apply_daemon(shell: &mut Shell, event: DaemonEvent) -> Vec<DaemonCall> {
    match event {
        DaemonEvent::Down => {
            shell.link = Link::Down;
            shell.notice = None;
            shell.devices = Catalog::default();
            shell.capture_backend = None;
            shell.capture_known = false;
            shell.panel_care = None;
            if shell.settings.is_open() && shell.page == Page::History {
                vec![history_call(shell)]
            } else {
                Vec::new()
            }
        }
        DaemonEvent::Snapshot(snapshot) => {
            shell.config_ok = Some(snapshot.config_errors.is_empty());
            shell.config_errors.clone_from(&snapshot.config_errors);
            shell.link = Link::Up(snapshot);
            shell.notice = None;
            let mut calls = refresh_devices(shell);
            if shell.settings.is_open() && shell.page == Page::History {
                calls.push(history_call(shell));
            }
            calls
        }
        DaemonEvent::State(state) => {
            apply_state(shell, state);
            Vec::new()
        }
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
            Vec::new()
        }
        DaemonEvent::Presets(presets) => {
            shell.presets_minutes = presets;
            Vec::new()
        }
        DaemonEvent::CallFailed(message) => {
            shell.notice = Some(message);
            Vec::new()
        }
        DaemonEvent::Devices(catalog) => {
            shell.devices = catalog;
            Vec::new()
        }
        DaemonEvent::Capture(backend) => {
            shell.capture_backend = backend;
            shell.capture_known = true;
            Vec::new()
        }
        DaemonEvent::Probe(view) => {
            if shell.settings.is_open() && shell.page == Page::Calibration {
                shell.calibration.view = Some(view);
            }
            Vec::new()
        }
        DaemonEvent::History(result) => {
            let message = match result {
                Ok(load) => HistMsg::Loaded(load),
                Err(text) => HistMsg::Failed(text),
            };
            history::apply(&mut shell.history, message)
        }
        DaemonEvent::Unit(view) => service::apply(&mut shell.service, SvcMsg::Unit(view)),
        DaemonEvent::Autostart(enabled) => {
            service::apply(&mut shell.service, SvcMsg::AutostartState(enabled))
        }
        DaemonEvent::Panel(care) => {
            shell.panel_care = care;
            Vec::new()
        }
    }
}

fn calibration_msg(shell: &mut Shell, message: CalMsg) -> Vec<DaemonCall> {
    let count = shell.editor.ignore_regions().len();
    let Some(change) = calibration::handle(&mut shell.calibration, count, message) else {
        return Vec::new();
    };
    settings_msg(shell, crate::edit_msg::SettingsMsg::Edit(change))
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
