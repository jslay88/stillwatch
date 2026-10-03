//! Picker data from a fake daemon on a private bus. Nothing here talks to the
//! session bus, opens a window, or blanks a display.

use std::sync::Arc;
use std::time::{Duration, Instant};

use stillwatch_core::backend::GamepadDevice;
use stillwatch_testkit::PrivateBus;
use stillwatchd::service::Service;
use stillwatchd::service::fake::FakeHandle;
use tokio::sync::mpsc;
use tokio::time::timeout;

use stillwatch_gui::{
    DaemonCall, DaemonEvent, gamepad_rows, output_rows, player_rows, player_value, watch,
};

const WAIT: Duration = Duration::from_secs(5);

#[tokio::test]
async fn pickers_read_outputs_gamepads_and_players_from_the_daemon() {
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
    let now = Instant::now();
    fake.update(|state| {
        state.outputs = vec!["HDMI-A-1".into(), "DP-1".into()];
        state.gamepads = vec![
            GamepadDevice {
                id: "event5".into(),
                name: "Drifty Pad".into(),
                ignored: false,
                last_activity: Some(now),
            },
            GamepadDevice {
                id: "event6".into(),
                name: "Quiet Pad".into(),
                ignored: false,
                last_activity: Some(now.checked_sub(Duration::from_secs(10)).unwrap()),
            },
        ];
        state.players = vec!["firefox.instance_1_42".into(), "spotify".into()];
    });
    let _service = Service::claim(server, Arc::clone(&fake) as _)
        .await
        .unwrap();
    let up = timeout(WAIT, next_snapshot(&mut events))
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(up, DaemonEvent::Snapshot(_)));

    fake.update(|state| {
        if let Some(pad) = state.gamepads.first_mut() {
            pad.last_activity = Some(Instant::now());
        }
    });
    calls.send(DaemonCall::RefreshDevices).await.unwrap();
    let catalog = timeout(WAIT, next_devices(&mut events))
        .await
        .unwrap()
        .unwrap();

    assert_eq!(catalog.outputs, ["HDMI-A-1", "DP-1"]);
    let outputs = output_rows(&["eDP-1".into()], &catalog.outputs);
    assert!(
        outputs
            .iter()
            .any(|row| row.value == "HDMI-A-1" && !row.selected)
    );
    assert!(
        outputs
            .iter()
            .any(|row| row.value == "eDP-1" && !row.connected)
    );

    let pads = gamepad_rows(&["missing".into()], &catalog.gamepads);
    let drifty = pads.iter().find(|row| row.value == "Drifty Pad").unwrap();
    let quiet = pads.iter().find(|row| row.value == "Quiet Pad").unwrap();
    assert_eq!(drifty.activity, Some(true));
    assert_eq!(quiet.activity, Some(false));
    assert!(
        pads.iter()
            .any(|row| row.value == "missing" && !row.connected)
    );

    assert_eq!(player_value("firefox.instance_1_42"), "firefox");
    let players = player_rows(&["vlc".into()], &catalog.players);
    assert!(
        players
            .iter()
            .any(|row| row.value == "firefox" && row.connected)
    );
    assert!(
        players
            .iter()
            .any(|row| row.value == "spotify" && !row.selected)
    );
    assert!(
        players
            .iter()
            .any(|row| row.value == "vlc" && !row.connected)
    );

    watch.abort();
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

async fn next_devices(
    events: &mut mpsc::Receiver<DaemonEvent>,
) -> Result<stillwatch_gui::Catalog, &'static str> {
    loop {
        match events.recv().await {
            Some(DaemonEvent::Devices(catalog)) => return Ok(catalog),
            Some(_) => {}
            None => return Err("watcher ended"),
        }
    }
}
