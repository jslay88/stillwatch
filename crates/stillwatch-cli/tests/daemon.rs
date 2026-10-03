//! Every daemon command, run in-process against a fake daemon on a private
//! session bus.

mod support;

use std::time::Duration;

use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp};
use stillwatch_cli::exit;
use stillwatch_cli::render::{self, Style};
use stillwatch_core::backend::BackendError;
use stillwatch_core::event::ControlCommand;
use stillwatch_core::history::{HistoryEntry, HistoryKind};
use stillwatch_core::state::{State, StatusSnapshot};
use stillwatch_ipc::json::{from_json, from_json_lines};
use stillwatch_ipc::status::StatusPayload;
use stillwatchd::service::{DaemonStatus, ReloadReport};
use support::{Daemon, stillwatch};

fn snoozed() -> DaemonStatus {
    DaemonStatus {
        capture_backend: Some("kwin".into()),
        ..DaemonStatus::new(StatusSnapshot {
            state: State::Snoozed,
            in_state: Duration::from_secs(90),
            snooze_remaining: Some(Duration::from_mins(44)),
            idle: true,
            locked: false,
            media_playing: false,
            last_detection: None,
        })
    }
}

#[tokio::test]
async fn status_prints_the_summary() {
    let Some(daemon) = Daemon::start().await.unwrap() else {
        return;
    };
    daemon.fake.update(|state| state.status = snoozed());
    let ran = stillwatch(daemon.address(), &["status"]).await.unwrap();
    ran.result.unwrap();
    assert_eq!(
        ran.out,
        render::status::render(&snoozed().into_payload(), &Style::plain(TimeZone::UTC))
    );
    assert!(
        ran.out
            .starts_with("state       snoozed for 1m 30s\nsnooze      44m left\n")
    );
}

#[tokio::test]
async fn status_json_is_one_status_payload() {
    let Some(daemon) = Daemon::start().await.unwrap() else {
        return;
    };
    daemon.fake.update(|state| state.status = snoozed());
    let ran = stillwatch(daemon.address(), &["status", "--json"])
        .await
        .unwrap();
    ran.result.unwrap();
    assert_eq!(ran.out.lines().count(), 1);
    assert_eq!(
        from_json::<StatusPayload>(&ran.out).unwrap(),
        snoozed().into_payload()
    );
    assert!(
        ran.out
            .starts_with(r#"{"state":"snoozed","state_seconds":90,"#)
    );
}

#[tokio::test]
async fn control_commands_reach_the_daemon_in_order() {
    let Some(daemon) = Daemon::start().await.unwrap() else {
        return;
    };
    let mut printed = String::new();
    for args in [
        &["snooze", "45m"][..],
        &["cancel-snooze"],
        &["pause"],
        &["resume"],
    ] {
        let ran = stillwatch(daemon.address(), args).await.unwrap();
        ran.result.unwrap();
        printed.push_str(&ran.out);
    }
    assert_eq!(
        printed,
        "snoozed for 45m\nsnooze cancelled\npaused\nresumed\n"
    );
    assert_eq!(
        daemon.fake.state().controls,
        [
            ControlCommand::Snooze(Duration::from_mins(45)),
            ControlCommand::CancelSnooze,
            ControlCommand::Pause,
            ControlCommand::Resume,
        ]
    );
}

#[tokio::test]
async fn a_snooze_outside_the_rules_is_refused() {
    let Some(daemon) = Daemon::start().await.unwrap() else {
        return;
    };
    let ran = stillwatch(daemon.address(), &["snooze", "13h"])
        .await
        .unwrap();
    assert_eq!(ran.error(), "snooze must be at most 720 minutes");
    assert_eq!(ran.exit_code(), exit::FAILED);
    assert_eq!(ran.out, "");
    assert_eq!(daemon.fake.state().controls.len(), 0);
}

#[tokio::test]
async fn daemon_failures_print_its_message() {
    let Some(daemon) = Daemon::start().await.unwrap() else {
        return;
    };
    daemon.fake.update(|state| {
        state.failure = Some(BackendError::Disconnected("state machine stopped".into()));
    });
    for args in [&["status"][..], &["pause"], &["reload"], &["history"]] {
        let ran = stillwatch(daemon.address(), args).await.unwrap();
        assert_eq!(
            ran.error(),
            "disconnected: state machine stopped",
            "{args:?}"
        );
        assert_eq!(ran.exit_code(), exit::FAILED);
    }
}

#[tokio::test]
async fn reload_prints_the_result() {
    let Some(daemon) = Daemon::start().await.unwrap() else {
        return;
    };
    let ran = stillwatch(daemon.address(), &["reload"]).await.unwrap();
    ran.result.unwrap();
    assert_eq!(ran.out, "config reloaded\n");

    let errors = vec![
        "stale.stale_percent: must be between 1 and 100, got 0".to_owned(),
        "action.command: must not be empty".to_owned(),
    ];
    daemon
        .fake
        .update(|state| state.reload = ReloadReport::rejected(errors.clone()));
    let ran = stillwatch(daemon.address(), &["reload"]).await.unwrap();
    assert_eq!(ran.out, format!("{}\n{}\n", errors[0], errors[1]));
    assert_eq!(
        ran.error(),
        "the config has 2 problems; stillwatchd kept the last good config"
    );
    assert_eq!(ran.exit_code(), exit::FAILED);

    for (errors, message) in [
        (vec!["bad".to_owned()], "the config has 1 problem"),
        (Vec::new(), "the config was rejected"),
    ] {
        daemon
            .fake
            .update(|state| state.reload = ReloadReport::rejected(errors));
        let ran = stillwatch(daemon.address(), &["reload"]).await.unwrap();
        assert!(ran.error().starts_with(message), "{}", ran.error());
    }
    assert_eq!(daemon.fake.state().reloads, 4);
}

fn history_around(now: Timestamp) -> Vec<HistoryEntry> {
    let ago = |hours| now - SignedDuration::from_hours(hours);
    vec![
        HistoryEntry::new(ago(3), HistoryKind::ConfigReload),
        HistoryEntry::transition(ago(1), State::Active, State::Monitoring),
    ]
}

#[tokio::test]
async fn history_prints_a_table_or_json_lines() {
    let Some(daemon) = Daemon::start().await.unwrap() else {
        return;
    };
    let entries = history_around(Timestamp::now());
    daemon.fake.update(|state| state.history = entries.clone());

    let ran = stillwatch(daemon.address(), &["history"]).await.unwrap();
    ran.result.unwrap();
    assert_eq!(
        ran.out,
        render::history::render(&entries, &Style::plain(TimeZone::UTC))
    );

    let ran = stillwatch(daemon.address(), &["history", "--since", "2h", "--json"])
        .await
        .unwrap();
    ran.result.unwrap();
    assert_eq!(ran.out.lines().count(), 1);
    assert_eq!(
        from_json_lines::<HistoryEntry>(&ran.out).unwrap(),
        entries[1..]
    );

    let asked = daemon.fake.state().history_since;
    assert_eq!(asked[0], Timestamp::MIN);
    let window = Timestamp::now().duration_since(asked[1]);
    assert!(window >= SignedDuration::from_hours(2), "{window:?}");
    assert!(window < SignedDuration::from_hours(2) + SignedDuration::from_mins(1));
}

#[tokio::test]
async fn empty_history_says_so() {
    let Some(daemon) = Daemon::start().await.unwrap() else {
        return;
    };
    let ran = stillwatch(daemon.address(), &["history"]).await.unwrap();
    ran.result.unwrap();
    assert_eq!(ran.out, "no history entries\n");
    let ran = stillwatch(daemon.address(), &["history", "--json"])
        .await
        .unwrap();
    ran.result.unwrap();
    assert_eq!(ran.out, "");
}

#[tokio::test]
async fn every_command_says_when_the_daemon_is_not_running() {
    let Some(bus) = stillwatch_testkit::PrivateBus::start().unwrap() else {
        return;
    };
    for args in [
        &["status"][..],
        &["status", "--json"],
        &["snooze", "45m"],
        &["cancel-snooze"],
        &["pause"],
        &["resume"],
        &["reload"],
        &["history"],
        &["probe", "--interval", "1s"],
    ] {
        let ran = stillwatch(bus.address(), args).await.unwrap();
        assert_eq!(
            ran.error(),
            "stillwatchd is not running; start it with systemctl --user start stillwatch",
            "{args:?}"
        );
        assert_eq!(ran.exit_code(), exit::NOT_RUNNING, "{args:?}");
        assert_eq!(ran.out, "");
    }
}
