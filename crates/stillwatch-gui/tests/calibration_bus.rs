//! Calibration probe against a fake daemon on a private bus. Nothing here
//! talks to the session bus, opens a window, or blanks a display.

use std::sync::Arc;
use std::time::Duration;

use stillwatch_core::stats::{BlockState, Threshold, ThresholdReason};
use stillwatch_ipc::probe::{ProbeOutput, ProbeSample};
use stillwatch_testkit::PrivateBus;
use stillwatchd::service::Service;
use stillwatchd::service::fake::FakeHandle;
use tokio::sync::mpsc;
use tokio::time::timeout;

use stillwatch_gui::{
    CalMsg, DaemonCall, DaemonEvent, Message, Page, ProbePace, Shell, update, watch,
};

const WAIT: Duration = Duration::from_secs(5);

fn sample() -> ProbeSample {
    let blocks = vec![
        BlockState::Persistent,
        BlockState::Changed,
        BlockState::Dark,
        BlockState::Ignored,
    ];
    let stats = stillwatch_core::stats::OutputStats::from_counts(
        "HDMI-A-1",
        stillwatch_core::stats::BlockCounts::from_states(&blocks),
        70,
    );
    ProbeSample {
        at: jiff::Timestamp::UNIX_EPOCH,
        threshold: Threshold::new(70, ThresholdReason::Normal),
        stale: false,
        outputs: vec![ProbeOutput {
            stats,
            columns: 2,
            rows: 2,
            blocks,
            width: 1920,
            height: 1080,
        }],
    }
}

#[tokio::test]
async fn the_page_starts_and_stops_a_probe_and_shows_its_blocks() {
    let Some(bus) = PrivateBus::start().unwrap() else {
        return;
    };
    let client = bus.connect().await.unwrap();
    let (calls, calls_rx) = mpsc::channel(8);
    let (events_tx, mut events) = mpsc::channel(16);
    let watch = tokio::spawn(watch(client, calls_rx, events_tx));

    let down = timeout(WAIT, events.recv()).await.unwrap().unwrap();
    assert_eq!(down, DaemonEvent::Down);

    let server = bus.connect().await.unwrap();
    let fake = Arc::new(FakeHandle::new());
    fake.update(|state| {
        state.sample = sample();
        state.status.capture_backend = Some("kwin".into());
    });
    let _service = Service::claim(server, Arc::clone(&fake) as _)
        .await
        .unwrap();

    let mut shell = Shell::new(vec![15, 60]);
    loop {
        let event = timeout(WAIT, events.recv()).await.unwrap().unwrap();
        let pending = update(&mut shell, Message::Daemon(event));
        send_all(&calls, pending).await.unwrap();
        if matches!(shell.link, stillwatch_gui::Link::Up(_)) && shell.capture_known {
            break;
        }
    }
    assert_eq!(shell.capture_backend.as_deref(), Some("kwin"));

    let pending = update(&mut shell, Message::OpenSettings);
    send_all(&calls, pending).await.unwrap();
    let pending = update(&mut shell, Message::Navigate(Page::Calibration));
    assert_eq!(pending, vec![DaemonCall::StartProbe { interval_ms: 5_000 }]);
    send_all(&calls, pending).await.unwrap();

    timeout(WAIT, fake.wait_for_probes(1)).await.unwrap();
    loop {
        let event = timeout(WAIT, events.recv()).await.unwrap().unwrap();
        let pending = update(&mut shell, Message::Daemon(event));
        send_all(&calls, pending).await.unwrap();
        if shell.calibration.view.is_some() {
            break;
        }
    }
    let view = shell.calibration.view.as_ref().unwrap();
    assert_eq!(
        view.outputs[0].cells,
        [
            BlockState::Persistent,
            BlockState::Changed,
            BlockState::Dark,
            BlockState::Ignored
        ]
    );
    assert_eq!(view.outputs[0].width, 1920);
    assert!(
        !view.outputs[0]
            .summary(&view.threshold_label())
            .contains("luma")
    );
    assert_eq!(fake.state().probe_intervals, [Duration::from_millis(5_000)]);

    let pending = update(
        &mut shell,
        Message::Calibration(CalMsg::Pace(ProbePace::OneSecond)),
    );
    assert_eq!(pending, vec![DaemonCall::StartProbe { interval_ms: 1_000 }]);
    send_all(&calls, pending).await.unwrap();

    let pending = update(&mut shell, Message::Navigate(Page::Settings));
    assert_eq!(pending, vec![DaemonCall::StopProbe]);
    send_all(&calls, pending).await.unwrap();
    timeout(WAIT, fake.wait_for_probes(0)).await.unwrap();
    assert!(shell.calibration.view.is_none());

    watch.abort();
}

async fn send_all(
    calls: &mpsc::Sender<DaemonCall>,
    pending: Vec<DaemonCall>,
) -> Result<(), mpsc::error::SendError<DaemonCall>> {
    for call in pending {
        calls.send(call).await?;
    }
    Ok(())
}
