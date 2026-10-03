use stillwatch_core::state::State;
use stillwatch_ipc::status::StatusPayload;

use super::update;
use crate::launch::LaunchMode;
use crate::page::Page;
use crate::shell::{
    DaemonCall, DaemonEvent, Link, Message, Shell, Snapshot, TrayAction, Visibility,
};

fn shell() -> Shell {
    Shell::new(vec![15, 60])
}

#[test]
fn pages_route_and_the_window_opens_without_a_daemon() {
    let mut shell = shell();
    assert_eq!(shell.link, Link::Down);
    assert_eq!(update(&mut shell, Message::OpenSettings), vec![]);
    assert_eq!(shell.settings, Visibility::Open);
    assert_eq!(update(&mut shell, Message::Navigate(Page::History)), vec![]);
    assert_eq!(shell.page, Page::History);
    assert_eq!(shell.settings, Visibility::Open);
    assert_eq!(update(&mut shell, Message::OpenSettings), vec![]);
    assert_eq!(shell.settings, Visibility::Focus);
    shell.settle_focus();
    assert_eq!(shell.settings, Visibility::Open);
    assert_eq!(update(&mut shell, Message::CloseSettings), vec![]);
    assert_eq!(shell.settings, Visibility::Closed);
    assert_eq!(shell.page, Page::History);
}

#[test]
fn first_tray_launch_stays_closed_and_a_second_one_focuses() {
    let mut shell = shell();
    shell.apply_launch(LaunchMode::Tray, true);
    assert_eq!(shell.settings, Visibility::Closed);
    assert_eq!(shell.prompt, Visibility::Closed);

    shell.apply_launch(LaunchMode::Settings, true);
    assert_eq!(shell.settings, Visibility::Open);
    shell.apply_launch(LaunchMode::Tray, false);
    assert_eq!(shell.settings, Visibility::Focus);

    shell.apply_launch(LaunchMode::Prompt, false);
    assert_eq!(shell.prompt, Visibility::Open);
    let _ = update(
        &mut shell,
        Message::BecamePrimary {
            first: false,
            mode: LaunchMode::Prompt,
        },
    );
    assert_eq!(shell.prompt, Visibility::Focus);
}

#[test]
fn menu_actions_become_daemon_calls_even_while_down() {
    let mut shell = shell();
    assert_eq!(shell.link, Link::Down);
    assert_eq!(
        update(
            &mut shell,
            Message::Tray(TrayAction::Snooze { minutes: 15 })
        ),
        vec![DaemonCall::Snooze { seconds: 900 }]
    );
    assert_eq!(
        update(&mut shell, Message::Tray(TrayAction::CancelSnooze)),
        vec![DaemonCall::CancelSnooze]
    );
    assert_eq!(
        update(&mut shell, Message::Tray(TrayAction::Pause)),
        vec![DaemonCall::Pause]
    );
    assert_eq!(
        update(&mut shell, Message::Tray(TrayAction::Resume)),
        vec![DaemonCall::Resume]
    );
    assert_eq!(shell.link, Link::Down);
}

#[test]
fn tray_opens_settings_and_quits() {
    let mut shell = shell();
    assert_eq!(
        update(&mut shell, Message::Tray(TrayAction::OpenSettings)),
        vec![]
    );
    assert_eq!(shell.settings, Visibility::Open);
    assert_eq!(update(&mut shell, Message::Tray(TrayAction::Quit)), vec![]);
    assert!(shell.quit);
    assert_eq!(update(&mut shell, Message::Quit), vec![]);
    assert!(shell.quit);
}

#[test]
fn daemon_up_down_and_snooze_time() {
    let mut shell = shell();
    let _ = update(
        &mut shell,
        Message::Daemon(DaemonEvent::Snapshot(Snapshot {
            state: State::Monitoring,
            snooze_remaining_seconds: None,
            config_errors: Vec::new(),
        })),
    );
    assert_eq!(
        shell.link,
        Link::Up(Snapshot {
            state: State::Monitoring,
            snooze_remaining_seconds: None,
            config_errors: Vec::new(),
        })
    );

    let _ = update(
        &mut shell,
        Message::Daemon(DaemonEvent::Snapshot(Snapshot {
            state: State::Snoozed,
            snooze_remaining_seconds: Some(90),
            config_errors: Vec::new(),
        })),
    );
    let _ = update(
        &mut shell,
        Message::Daemon(DaemonEvent::State(State::Snoozed)),
    );
    assert_eq!(
        shell.link,
        Link::Up(Snapshot {
            state: State::Snoozed,
            snooze_remaining_seconds: Some(90),
            config_errors: Vec::new(),
        })
    );
    let _ = update(
        &mut shell,
        Message::Daemon(DaemonEvent::State(State::Active)),
    );
    assert_eq!(
        shell.link,
        Link::Up(Snapshot {
            state: State::Active,
            snooze_remaining_seconds: None,
            config_errors: Vec::new(),
        })
    );

    let _ = update(&mut shell, Message::Daemon(DaemonEvent::Down));
    assert_eq!(shell.link, Link::Down);
    assert_eq!(shell.notice, None);

    let _ = update(
        &mut shell,
        Message::Daemon(DaemonEvent::CallFailed("stillwatchd is not running".into())),
    );
    assert_eq!(shell.notice.as_deref(), Some("stillwatchd is not running"));
    let _ = update(&mut shell, Message::Daemon(DaemonEvent::Down));
    assert_eq!(shell.notice, None);
}

#[test]
fn config_reload_keeps_or_replaces_presets() {
    let mut shell = shell();
    let _ = update(
        &mut shell,
        Message::Daemon(DaemonEvent::Config {
            ok: false,
            errors: vec!["stale.stale_percent: must be 1-100".into()],
        }),
    );
    assert_eq!(shell.config_ok, Some(false));
    assert_eq!(shell.presets_minutes, vec![15, 60]);
    let _ = update(
        &mut shell,
        Message::Daemon(DaemonEvent::Presets(vec![20, 40])),
    );
    assert_eq!(shell.presets_minutes, vec![20, 40]);
}

#[test]
fn status_payload_maps_onto_the_snapshot() {
    let status = StatusPayload {
        snooze_remaining_seconds: Some(12),
        config_errors: vec!["stale.stale_percent: must be between 1 and 100, got 0".into()],
        ..StatusPayload::new(State::Blanked)
    };
    assert_eq!(
        Snapshot::from_status(&status),
        Snapshot {
            state: State::Blanked,
            snooze_remaining_seconds: Some(12),
            config_errors: vec!["stale.stale_percent: must be between 1 and 100, got 0".into()],
        }
    );
}
