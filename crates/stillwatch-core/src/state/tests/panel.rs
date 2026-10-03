//! Panel care commands from the state machine.

use std::time::Duration;

use super::{config, effects};
use crate::command::{Command, HookKind};
use crate::event::{Event, PowerKind};
use crate::mocks::Harness;
use crate::panel::PanelRecord;
use crate::prompt::Reminder;
use crate::time::{Clock, TimerId};

fn power(on: bool, kind: PowerKind) -> Event {
    Event::DisplayPower {
        output: "HDMI-A-1".into(),
        on,
        kind,
    }
}

#[test]
fn the_reminder_is_one_notification_until_its_snooze_passes() {
    let mut h = Harness::with_config(&config(|c| c.panel_care.reminder_hours = 1));
    let armed = h.send(power(true, PowerKind::Dpms));
    assert!(armed.iter().any(|command| matches!(
        command,
        Command::SetTimer {
            id: TimerId::PanelCareReminder,
            after,
        } if *after == Duration::from_hours(1) + Duration::from_secs(1)
    )));

    let commands = h.advance(Duration::from_hours(1) + Duration::from_secs(1));
    assert!(commands.contains(&Command::Notify(Reminder::PanelCare {
        screen_on: Duration::from_hours(1) + Duration::from_secs(1),
    })));
    assert!(
        h.advance(Duration::from_mins(30))
            .iter()
            .all(|command| { !matches!(command, Command::Notify(_)) })
    );

    let again = h.advance(Duration::from_mins(30));
    assert!(
        again
            .iter()
            .any(|command| matches!(command, Command::Notify(_)))
    );
}

#[test]
fn trigger_cmd_runs_at_blank_time_only_when_due() {
    let mut h = Harness::with_config(&config(|c| {
        c.panel_care.reminder_hours = 1;
        c.panel_care.trigger_cmd = "pixel-clean".into();
        c.action.on_blank_cmd = "tv-off".into();
    }));
    let early = h.to_blanked();
    assert!(early.contains(&Command::RunHook(HookKind::OnBlank)));
    assert!(!early.contains(&Command::RunHook(HookKind::PanelCareTrigger)));

    h.input();
    h.send(power(true, PowerKind::Dpms));
    h.advance(Duration::from_hours(1) + Duration::from_secs(1));
    let due = effects(&h.to_blanked());
    assert!(due.contains(&Command::RunHook(HookKind::OnBlank)));
    assert!(due.contains(&Command::RunHook(HookKind::PanelCareTrigger)));
}

#[test]
fn overlay_does_not_make_the_trigger_reset() {
    let mut h = Harness::with_config(&config(|c| {
        c.panel_care.reminder_hours = 1;
        c.panel_care.trigger_cmd = "pixel-clean".into();
    }));
    h.send(power(true, PowerKind::Dpms));
    h.advance(Duration::from_mins(30));
    h.send(power(false, PowerKind::Overlay));
    h.advance(Duration::from_mins(31));
    let commands = h.to_blanked();
    assert!(commands.contains(&Command::RunHook(HookKind::PanelCareTrigger)));
    let record = h.machine().panel_record(h.clock().now()).expect("tracked");
    assert!(record.screen_on_seconds >= 60 * 60);
    assert_eq!(record.overlay_uses, 1);
}

#[test]
fn disabled_panel_care_tracks_nothing() {
    let mut h = Harness::with_config(&config(|c| {
        c.panel_care.enabled = false;
        c.panel_care.trigger_cmd = "pixel-clean".into();
    }));
    assert_eq!(h.send(power(true, PowerKind::Dpms)), vec![]);
    assert!(h.machine().panel_record(h.clock().now()).is_none());
    h.advance(Duration::from_hours(5));
    assert!(
        !h.to_blanked()
            .contains(&Command::RunHook(HookKind::PanelCareTrigger))
    );
}

#[test]
fn restored_counters_survive_on_the_machine() {
    let mut h = Harness::new();
    h.restore_panel(PanelRecord {
        screen_on_seconds: 50,
        last_standby: None,
        overlay_uses: 4,
    });
    let record = h.machine().panel_record(h.clock().now()).expect("tracked");
    assert_eq!(record.screen_on_seconds, 50);
    assert_eq!(record.overlay_uses, 4);
}
