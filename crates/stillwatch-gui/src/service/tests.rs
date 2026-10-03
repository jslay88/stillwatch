use std::fs;
use std::path::Path;

use jiff::Timestamp;
use stillwatch_ipc::status::PanelCareStatus;

use super::autostart::{self, FILE_NAME};
use super::journal::{self, FULL_ARGS, RECENT_ARGS};
use super::unit::{RunState, UnitView, enabled_at_login, unit_sentence};
use super::{SvcMsg, UnitOp, apply, panel_lines};
use crate::model::update;
use crate::page::Page;
use crate::shell::{DaemonCall, Message, Shell, Visibility};

#[test]
fn unit_state_maps_active_enabled_and_failed() {
    assert_eq!(RunState::from_active("active"), RunState::Active);
    assert_eq!(RunState::from_active("inactive"), RunState::Inactive);
    assert_eq!(RunState::from_active("failed"), RunState::Failed);
    assert_eq!(
        RunState::from_active("activating"),
        RunState::Other("activating".into())
    );
    assert!(enabled_at_login("enabled"));
    assert!(enabled_at_login("enabled-runtime"));
    assert!(enabled_at_login("linked"));
    assert!(enabled_at_login("indirect"));
    assert!(!enabled_at_login("disabled"));
    assert!(!enabled_at_login("static"));
    assert!(!enabled_at_login("masked"));

    let running = UnitView::ready("active", "enabled", Vec::new());
    assert_eq!(
        unit_sentence(&running),
        "stillwatch.service is active (enabled)."
    );
    assert_eq!(running.journal(), [] as [&str; 0]);

    let failed = UnitView::ready("failed", "enabled", vec!["boom".into(), "line 2".into()]);
    assert!(unit_sentence(&failed).contains("failed"));
    assert_eq!(failed.journal(), ["boom", "line 2"]);
    assert!(RunState::from_active("failed").is_failed());

    assert_eq!(
        unit_sentence(&UnitView::Missing),
        "stillwatch.service is not installed."
    );
    assert_eq!(
        unit_sentence(&UnitView::Error("no manager".into())),
        "no manager"
    );
}

#[test]
fn panel_lines_use_the_status_counters() {
    assert_eq!(
        panel_lines(None),
        ["Panel care has not reported yet.".to_owned()]
    );
    let care = PanelCareStatus {
        screen_on_seconds: 3 * 3600 + 12 * 60,
        last_standby: Some(Timestamp::from_second(1_790_000_000).unwrap()),
        overlay_uses: 2,
    };
    let lines = panel_lines(Some(&care));
    assert_eq!(lines[0], "Screen on: 3h 12m");
    assert!(lines[1].starts_with("Last standby: "), "{lines:?}");
    assert_ne!(lines[1], "Last standby: never");
    assert_eq!(lines[2], "Overlay uses: 2");

    let never = PanelCareStatus {
        screen_on_seconds: 0,
        last_standby: None,
        overlay_uses: 0,
    };
    assert_eq!(panel_lines(Some(&never))[1], "Last standby: never");
}

#[test]
fn autostart_toggle_writes_and_removes_the_desktop_file() {
    let home = tempfile::tempdir().unwrap();
    let config_home = home.path();
    assert!(!autostart::is_enabled(config_home));
    autostart::set_enabled(config_home, true).unwrap();
    let path = autostart::desktop_path(config_home);
    assert_eq!(path, config_home.join("autostart").join(FILE_NAME));
    assert!(path.is_file());
    let text = fs::read_to_string(&path).unwrap();
    assert_eq!(text, autostart::template());
    assert!(text.contains("Exec=stillwatch-gui"), "{text}");
    assert!(text.contains("X-GNOME-Autostart-enabled=true"), "{text}");
    assert!(autostart::is_enabled(config_home));

    autostart::set_enabled(config_home, false).unwrap();
    assert!(!path.exists());
    assert!(!autostart::is_enabled(config_home));
    autostart::set_enabled(config_home, false).unwrap();
}

#[test]
fn config_home_prefers_xdg_then_home() {
    assert_eq!(
        autostart::config_home_from(Some(Path::new("/tmp/xdg")), Some(Path::new("/home/me"))),
        Some(Path::new("/tmp/xdg").to_path_buf())
    );
    assert_eq!(
        autostart::config_home_from(Some(Path::new("")), Some(Path::new("/home/me"))),
        Some(Path::new("/home/me/.config").to_path_buf())
    );
    assert_eq!(autostart::config_home_from(None, None), None);
}

#[test]
fn the_service_page_asks_for_the_unit_and_the_autostart_file() {
    let mut shell = Shell::new(Vec::new());
    shell.settings = Visibility::Open;
    assert_eq!(
        update(&mut shell, Message::Navigate(Page::Service)),
        vec![DaemonCall::RefreshUnit, DaemonCall::ReadAutostart]
    );
    assert_eq!(
        update(&mut shell, Message::Service(SvcMsg::Start)),
        vec![DaemonCall::Unit(UnitOp::Start)]
    );
    assert_eq!(
        update(&mut shell, Message::Service(SvcMsg::Enable)),
        vec![DaemonCall::Unit(UnitOp::Enable)]
    );
    assert_eq!(
        update(&mut shell, Message::Service(SvcMsg::Autostart(true))),
        vec![DaemonCall::SetAutostart(true)]
    );
    assert_eq!(
        update(&mut shell, Message::Service(SvcMsg::OpenLog)),
        vec![DaemonCall::OpenJournal]
    );
    let calls = update(
        &mut shell,
        Message::Service(SvcMsg::Unit(UnitView::ready(
            "inactive",
            "disabled",
            Vec::new(),
        ))),
    );
    assert_eq!(calls, [] as [DaemonCall; 0]);
    assert_eq!(
        unit_sentence(&shell.service.unit),
        "stillwatch.service is inactive (disabled)."
    );
}

#[test]
fn journal_helpers_do_not_touch_the_user_journal() {
    assert!(RECENT_ARGS.contains(&"-n"));
    assert!(RECENT_ARGS.contains(&"50"));
    assert!(RECENT_ARGS.contains(&"stillwatch.service"));
    assert!(FULL_ARGS.contains(&"-e"));
    assert_eq!(journal::parse_lines(" one \n\n two\n"), [" one", " two"]);
    assert_eq!(journal::command_lines("echo", &["alpha"]), ["alpha"]);
    assert_eq!(
        journal::command_lines("false", &[]),
        ["journalctl failed".to_owned()]
    );
    journal::open_with(
        &[("/no/such/stillwatch-terminal", &[]), ("true", &[])],
        &["ignored"],
    )
    .unwrap();
    let err = journal::open_with(&[("/no/such/stillwatch-terminal", &[])], &[]).unwrap_err();
    assert!(err.contains("No terminal"), "{err}");
}

#[test]
fn apply_stores_autostart() {
    let mut page = super::ServicePage::default();
    assert_eq!(
        apply(&mut page, SvcMsg::AutostartState(true)),
        [] as [DaemonCall; 0]
    );
    assert_eq!(page.autostart, Some(true));
}
