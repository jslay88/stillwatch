use stillwatch_core::state::State;

use super::{TrayIcon, format_remaining, icon_for, icon_for_state, status_text, tray_model};
use crate::shell::{Link, Shell, Snapshot, TrayAction};

#[test]
fn every_state_has_an_icon_distinct_from_daemon_down() {
    assert_eq!(icon_for(&Link::Down), TrayIcon::Down);
    let mut seen = vec![TrayIcon::Down];
    for state in State::ALL {
        let icon = icon_for_state(state);
        assert!(!seen.contains(&icon), "{state} shares an icon");
        seen.push(icon);
        let shell = Shell {
            link: Link::Up(Snapshot {
                state,
                snooze_remaining_seconds: None,
                config_errors: Vec::new(),
            }),
            ..Shell::new(vec![15])
        };
        assert_eq!(tray_model(&shell).icon, icon);
    }
}

#[test]
fn tooltip_names_the_state_and_snooze_time_left() {
    assert_eq!(status_text(&Link::Down), "stillwatchd is not running");
    assert_eq!(
        status_text(&Link::Up(Snapshot {
            state: State::Active,
            snooze_remaining_seconds: None,
            config_errors: Vec::new(),
        })),
        "active"
    );
    assert_eq!(
        status_text(&Link::Up(Snapshot {
            state: State::Snoozed,
            snooze_remaining_seconds: Some(90),
            config_errors: Vec::new(),
        })),
        "snoozed, 1m 30s left"
    );
    let shell = Shell {
        link: Link::Up(Snapshot {
            state: State::Paused,
            snooze_remaining_seconds: None,
            config_errors: Vec::new(),
        }),
        ..Shell::new(Vec::new())
    };
    let model = tray_model(&shell);
    assert_eq!(model.title, "Stillwatch");
    assert_eq!(model.description, "paused");
}

#[test]
fn menu_lists_each_preset_and_swaps_pause_for_resume() {
    let model = tray_model(&Shell::new(vec![1, 15]));
    assert_eq!(
        model
            .entries
            .iter()
            .map(|entry| (entry.label.as_str(), entry.action))
            .collect::<Vec<_>>(),
        vec![
            ("Snooze 1 minute", TrayAction::Snooze { minutes: 1 }),
            ("Snooze 15 minutes", TrayAction::Snooze { minutes: 15 }),
            ("Cancel snooze", TrayAction::CancelSnooze),
            ("Pause", TrayAction::Pause),
            ("Settings", TrayAction::OpenSettings),
            ("Quit", TrayAction::Quit),
        ]
    );

    let paused = Shell {
        link: Link::Up(Snapshot {
            state: State::Paused,
            snooze_remaining_seconds: None,
            config_errors: Vec::new(),
        }),
        ..Shell::new(vec![15])
    };
    let actions: Vec<_> = tray_model(&paused)
        .entries
        .iter()
        .map(|entry| entry.action)
        .collect();
    assert!(actions.contains(&TrayAction::Resume));
    assert!(!actions.contains(&TrayAction::Pause));
}

#[test]
fn remaining_time_uses_hours_then_minutes_then_seconds() {
    assert_eq!(format_remaining(0), "0s");
    assert_eq!(format_remaining(59), "59s");
    assert_eq!(format_remaining(60), "1m 0s");
    assert_eq!(format_remaining(3600), "1h 0m");
    assert_eq!(format_remaining(3661), "1h 1m");
}
