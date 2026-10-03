//! Tray actions against a fake daemon, and the single-instance name, both on
//! a private bus.

use std::sync::Arc;
use std::time::Duration;

use stillwatch_core::event::ControlCommand;
use stillwatch_core::state::State;
use stillwatch_ipc::BUS_NAME;
use stillwatch_ipc::proxy::StillwatchProxy;
use stillwatch_testkit::PrivateBus;
use stillwatchd::service::Service;
use stillwatchd::service::fake::FakeHandle;
use tokio::sync::mpsc;
use tokio::time::timeout;
use zbus::fdo::DBusProxy;
use zbus::proxy::CacheProperties;

use stillwatch_gui::{
    Claim, DaemonCall, DaemonEvent, GUI_BUS_NAME, LaunchMode, Message, Shell, TrayAction, claim,
    dispatch, update, watch,
};

const WAIT: Duration = Duration::from_secs(5);

#[tokio::test]
async fn tray_model_snooze_and_pause_reach_the_daemon() {
    let Some(bus) = PrivateBus::start().unwrap() else {
        return;
    };
    let server = bus.connect().await.unwrap();
    let client = bus.connect().await.unwrap();
    let fake = Arc::new(FakeHandle::new());
    let _service = Service::claim(server, Arc::clone(&fake) as _)
        .await
        .unwrap();
    let proxy = StillwatchProxy::builder(&client)
        .cache_properties(CacheProperties::No)
        .build()
        .await
        .unwrap();

    let mut shell = Shell::new(vec![15, 60, 180]);
    for action in [
        TrayAction::Snooze { minutes: 15 },
        TrayAction::Pause,
        TrayAction::Resume,
        TrayAction::CancelSnooze,
    ] {
        for call in update(&mut shell, Message::Tray(action)) {
            dispatch(&proxy, call).await.unwrap();
        }
    }

    assert_eq!(
        fake.state().controls,
        vec![
            ControlCommand::Snooze(Duration::from_mins(15)),
            ControlCommand::Pause,
            ControlCommand::Resume,
            ControlCommand::CancelSnooze,
        ]
    );
}

#[tokio::test]
async fn watcher_reconnects_and_forwards_a_tray_call() {
    let Some(bus) = PrivateBus::start().unwrap() else {
        return;
    };
    let client = bus.connect().await.unwrap();
    let (calls, calls_rx) = mpsc::channel(4);
    let (events_tx, mut events) = mpsc::channel(8);
    let watch = tokio::spawn(watch(client, calls_rx, events_tx));

    let down = timeout(WAIT, events.recv()).await.unwrap().unwrap();
    assert_eq!(down, DaemonEvent::Down);

    let server = bus.connect().await.unwrap();
    let fake = Arc::new(FakeHandle::new());
    let service = Service::claim(server, Arc::clone(&fake) as _)
        .await
        .unwrap();
    let up = timeout(WAIT, events.recv()).await.unwrap().unwrap();
    match up {
        DaemonEvent::Snapshot(snapshot) => assert_eq!(snapshot.state, State::Active),
        other => panic!("expected a snapshot, got {other:?}"),
    }

    calls.send(DaemonCall::Pause).await.unwrap();
    timeout(WAIT, async {
        loop {
            if fake.state().controls == vec![ControlCommand::Pause] {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    drop(service);
    let down = timeout(WAIT, next_down(&mut events))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(down, DaemonEvent::Down);

    let server = bus.connect().await.unwrap();
    let _service = Service::claim(server, fake).await.unwrap();
    let up = timeout(WAIT, next_snapshot(&mut events))
        .await
        .unwrap()
        .unwrap();
    match up {
        DaemonEvent::Snapshot(snapshot) => assert_eq!(snapshot.state, State::Active),
        other => panic!("expected a snapshot after restart, got {other:?}"),
    }

    watch.abort();
}

#[tokio::test]
async fn a_second_process_hands_the_window_to_the_first() {
    let Some(bus) = PrivateBus::start().unwrap() else {
        return;
    };
    let primary = bus.connect().await.unwrap();
    let secondary = bus.connect().await.unwrap();
    let (tx, mut rx) = mpsc::channel(2);
    let owned = claim(&primary, LaunchMode::Tray, tx).await.unwrap();
    assert_eq!(owned, Claim::Primary);

    let dbus = DBusProxy::new(&secondary).await.unwrap();
    assert!(
        dbus.name_has_owner(GUI_BUS_NAME.try_into().unwrap())
            .await
            .unwrap()
    );
    assert_ne!(GUI_BUS_NAME, BUS_NAME);

    let (other_tx, _other_rx) = mpsc::channel(1);
    let handed = claim(&secondary, LaunchMode::Settings, other_tx)
        .await
        .unwrap();
    assert_eq!(handed, Claim::HandedOff);
    assert_eq!(
        timeout(WAIT, rx.recv()).await.unwrap(),
        Some(LaunchMode::Settings)
    );
}

async fn next_down(events: &mut mpsc::Receiver<DaemonEvent>) -> Result<DaemonEvent, &'static str> {
    loop {
        match events.recv().await {
            Some(DaemonEvent::Down) => return Ok(DaemonEvent::Down),
            Some(_) => {}
            None => return Err("watcher ended"),
        }
    }
}

async fn next_snapshot(
    events: &mut mpsc::Receiver<DaemonEvent>,
) -> Result<DaemonEvent, &'static str> {
    loop {
        match events.recv().await {
            Some(event @ DaemonEvent::Snapshot(_)) => return Ok(event),
            Some(_) => {}
            None => return Err("watcher ended"),
        }
    }
}
