use std::sync::Arc;
use std::time::Duration;

use stillwatch_core::backend::{BackendError, Blanker};
use stillwatch_core::event::{Event, PowerKind};
use stillwatch_core::mocks::RecordingSink;
use tempfile::TempDir;

use super::blanker::power_state;
use super::fixtures::{PG48UQ, dell, drm_root};
use super::mock::{Call, FakeBus};
use super::*;

struct Rig {
    bus: Arc<FakeBus>,
    blanker: Arc<DdcBlanker>,
    _drm: TempDir,
}

fn rig(bus: FakeBus, connected: &[(&str, &[u8])]) -> Rig {
    rig_with(bus, connected, Timing::default())
}

fn rig_with(bus: FakeBus, connected: &[(&str, &[u8])], timing: Timing) -> Rig {
    let bus = Arc::new(bus);
    let drm = drm_root(connected);
    let blanker = DdcBlanker::with_parts(bus.clone(), DrmConnectors::new(drm.path()), timing);
    Rig {
        bus,
        blanker: Arc::new(blanker),
        _drm: drm,
    }
}

/// A PG48UQ on `i2c-3` driving `HDMI-A-1`.
fn pg48uq() -> Rig {
    rig(
        FakeBus::new().with_display("i2c-3", &PG48UQ),
        &[("HDMI-A-1", &PG48UQ)],
    )
}

/// The PG48UQ plus a Dell on `i2c-2` driving `DP-1`.
fn two_displays() -> Rig {
    rig(
        FakeBus::new()
            .with_display("i2c-2", &dell())
            .with_display("i2c-3", &PG48UQ),
        &[("HDMI-A-1", &PG48UQ), ("DP-1", &dell())],
    )
}

fn names(outputs: &[&str]) -> Vec<String> {
    outputs.iter().map(|&o| o.to_owned()).collect()
}

fn power(output: &str, on: bool) -> Event {
    Event::DisplayPower {
        output: output.into(),
        on,
        kind: PowerKind::Ddc,
    }
}

#[tokio::test]
async fn blank_writes_standby_and_unblank_writes_on() {
    let rig = pg48uq();
    let hdmi = names(&["HDMI-A-1"]);
    rig.blanker.blank(&hdmi).await.unwrap();
    assert_eq!(rig.bus.power("i2c-3"), Some(STANDBY_POWER_MODE));
    rig.blanker.unblank(&hdmi).await.unwrap();
    assert_eq!(rig.bus.power("i2c-3"), Some(POWER_ON));
    assert_eq!(
        rig.bus.calls(),
        vec![
            Call::Scan,
            Call::Set("i2c-3".into(), VCP_POWER_MODE, 0x04),
            Call::Set("i2c-3".into(), VCP_POWER_MODE, 0x01),
        ]
    );
}

#[tokio::test]
async fn empty_lists_mean_every_display() {
    let rig = two_displays();
    rig.blanker.blank(&[]).await.unwrap();
    rig.blanker.unblank(&[]).await.unwrap();
    assert_eq!(
        rig.bus.writes(),
        [
            ("i2c-2".into(), 0x04),
            ("i2c-3".into(), 0x04),
            ("i2c-2".into(), 0x01),
            ("i2c-3".into(), 0x01),
        ]
    );
}

#[tokio::test]
async fn only_named_outputs_are_touched() {
    let rig = two_displays();
    rig.blanker
        .blank(&names(&["DP-1", "HDMI-A-1"]))
        .await
        .unwrap();
    rig.blanker.unblank(&names(&["DP-1"])).await.unwrap();
    assert_eq!(rig.bus.power("i2c-2"), Some(POWER_ON));
    assert_eq!(rig.bus.power("i2c-3"), Some(STANDBY_POWER_MODE));
}

#[tokio::test]
async fn unblanking_what_wasnt_blanked_is_a_no_op() {
    let rig = pg48uq();
    rig.blanker.unblank(&names(&["HDMI-A-1"])).await.unwrap();
    rig.blanker.unblank(&[]).await.unwrap();
    assert_eq!(rig.bus.calls(), Vec::new());
}

#[tokio::test]
async fn one_missing_output_doesnt_stop_the_others() {
    let rig = pg48uq();
    let error = rig
        .blanker
        .blank(&names(&["DP-9", "HDMI-A-1"]))
        .await
        .unwrap_err();
    assert_eq!(
        error,
        BackendError::NotFound(
            "no DDC/CI display for output DP-9: it isn't connected or has no EDID".into()
        )
    );
    assert_eq!(rig.bus.power("i2c-3"), Some(STANDBY_POWER_MODE));
    rig.blanker.unblank(&[]).await.unwrap();
    assert_eq!(rig.bus.power("i2c-3"), Some(POWER_ON));
}

#[tokio::test]
async fn missing_i2c_access_explains_the_fix() {
    let rig = rig(
        FakeBus::new().with_denied("/dev/i2c-3"),
        &[("HDMI-A-1", &PG48UQ)],
    );
    let error = rig.blanker.blank(&names(&["HDMI-A-1"])).await.unwrap_err();
    let BackendError::PermissionDenied(message) = error else {
        panic!("expected PermissionDenied, got {error:?}");
    };
    assert!(message.contains("/dev/i2c-3"), "{message}");
    assert!(message.contains("`i2c` group"), "{message}");
    assert_eq!(rig.bus.writes(), Vec::new());
}

#[tokio::test]
async fn scan_failures_are_reported() {
    let rig = pg48uq();
    rig.bus
        .fail_scans(DdcError::Unavailable("no i2c bus belongs to a GPU".into()));
    assert_eq!(
        rig.blanker.blank(&[]).await,
        Err(BackendError::Unavailable(
            "DDC/CI unavailable: no i2c bus belongs to a GPU".into()
        ))
    );
}

#[tokio::test(start_paused = true)]
async fn flaky_writes_are_retried() {
    let rig = pg48uq();
    rig.bus.fail_next(2, "NAK");
    rig.blanker.blank(&names(&["HDMI-A-1"])).await.unwrap();
    assert_eq!(rig.bus.writes().len(), 3);
    assert_eq!(rig.bus.power("i2c-3"), Some(STANDBY_POWER_MODE));
}

#[tokio::test(start_paused = true)]
async fn writes_fail_after_the_last_retry() {
    let rig = pg48uq();
    rig.bus.fail_next(ATTEMPTS as usize, "NAK");
    assert_eq!(
        rig.blanker.blank(&names(&["HDMI-A-1"])).await,
        Err(BackendError::Io(
            "writing VCP 0xd6 = 0x04 to HDMI-A-1 failed: NAK".into()
        ))
    );
    assert_eq!(rig.bus.writes().len(), 3);
    rig.blanker.unblank(&[]).await.unwrap();
    assert_eq!(rig.bus.writes().len(), 3, "a failed blank isn't tracked");
}

#[tokio::test(start_paused = true)]
async fn failed_wakes_are_reported() {
    let rig = pg48uq();
    rig.blanker.blank(&[]).await.unwrap();
    rig.bus.fail_next(ATTEMPTS as usize, "bus busy");
    assert_eq!(
        rig.blanker.unblank(&[]).await,
        Err(BackendError::Io(
            "writing VCP 0xd6 = 0x01 to HDMI-A-1 failed: bus busy".into()
        ))
    );
}

#[tokio::test]
async fn hung_writes_time_out() {
    let timing = Timing {
        io_timeout: Duration::from_millis(20),
        ..Timing::default()
    };
    let rig = rig_with(
        FakeBus::new().with_display("i2c-3", &PG48UQ),
        &[("HDMI-A-1", &PG48UQ)],
        timing,
    );
    rig.bus.set_delay(Duration::from_millis(300));
    assert_eq!(
        rig.blanker.blank(&[]).await,
        Err(BackendError::Io("DDC/CI write timed out after 20ms".into()))
    );
    assert_eq!(rig.bus.writes().len(), 1, "timeouts aren't retried");
}

#[tokio::test]
async fn a_crashed_worker_is_an_error() {
    let rig = pg48uq();
    rig.bus.panic_next();
    let error = rig.blanker.blank(&[]).await.unwrap_err();
    assert!(
        matches!(&error, BackendError::Unavailable(m) if m.contains("write worker died")),
        "{error:?}"
    );
}

/// Starts `watch` on its own task and returns what it reports.
fn watching(rig: &Rig) -> Arc<RecordingSink> {
    let sink = Arc::new(RecordingSink::new());
    let blanker = Arc::clone(&rig.blanker);
    let events = Arc::clone(&sink);
    tokio::spawn(async move { blanker.watch(events).await });
    sink
}

async fn wait(seconds: u64) {
    tokio::time::sleep(Duration::from_secs(seconds)).await;
}

#[tokio::test(start_paused = true)]
async fn power_mode_is_polled_only_while_blanked() {
    let rig = pg48uq();
    let sink = watching(&rig);
    wait(120).await;
    assert_eq!(rig.bus.reads(), 0);

    rig.blanker.blank(&[]).await.unwrap();
    wait(5).await;
    assert_eq!(rig.bus.reads(), 0, "the first read waits one interval");
    wait(10).await;
    assert_eq!(rig.bus.reads(), 1);
    assert_eq!(sink.take(), [power("HDMI-A-1", false)]);
    wait(30).await;
    assert_eq!(rig.bus.reads(), 4);
    assert!(sink.is_empty(), "only changes are reported");

    rig.bus.set_power("i2c-3", POWER_ON);
    wait(10).await;
    assert_eq!(sink.take(), [power("HDMI-A-1", true)]);

    rig.blanker.unblank(&[]).await.unwrap();
    let reads = rig.bus.reads();
    wait(120).await;
    assert_eq!(rig.bus.reads(), reads);
    assert!(sink.is_empty());
}

#[tokio::test(start_paused = true)]
async fn reblanking_reports_off_again() {
    let rig = pg48uq();
    let sink = watching(&rig);
    rig.blanker.blank(&[]).await.unwrap();
    wait(11).await;
    rig.bus.set_power("i2c-3", POWER_ON);
    wait(10).await;
    assert_eq!(
        sink.take(),
        [power("HDMI-A-1", false), power("HDMI-A-1", true)]
    );
    rig.blanker.blank(&[]).await.unwrap();
    wait(10).await;
    assert_eq!(sink.take(), [power("HDMI-A-1", false)]);
}

#[tokio::test(start_paused = true)]
async fn silent_displays_count_as_off() {
    let rig = pg48uq();
    rig.bus.silent_in_standby();
    let sink = watching(&rig);
    rig.blanker.blank(&[]).await.unwrap();
    wait(11).await;
    assert_eq!(sink.take(), [power("HDMI-A-1", false)]);
    assert_eq!(rig.bus.reads(), ATTEMPTS as usize, "reads are retried");
}

#[tokio::test(start_paused = true)]
async fn undefined_power_modes_are_ignored() {
    let rig = pg48uq();
    let sink = watching(&rig);
    rig.blanker.blank(&[]).await.unwrap();
    rig.bus.set_power("i2c-3", 0x07);
    wait(31).await;
    assert_eq!(rig.bus.reads(), 3);
    assert!(sink.is_empty());
}

#[tokio::test(start_paused = true)]
async fn each_blanked_display_is_polled() {
    let rig = two_displays();
    let sink = watching(&rig);
    rig.blanker.blank(&[]).await.unwrap();
    wait(11).await;
    assert_eq!(
        sink.take(),
        [power("DP-1", false), power("HDMI-A-1", false)]
    );
}

#[test]
fn power_modes_decode_per_mccs() {
    assert_eq!(power_state(0x01), Some(true));
    for off in 0x02..=0x05 {
        assert_eq!(power_state(off), Some(false));
    }
    assert_eq!(power_state(0x0101), Some(true), "only the low byte counts");
    assert_eq!(power_state(0x00), None);
    assert_eq!(power_state(0x06), None);
}

#[test]
fn the_system_blanker_opens_nothing_up_front() {
    let blanker: Arc<dyn Blanker> = Arc::new(DdcBlanker::default());
    drop(blanker);
    assert_eq!(STANDBY_POWER_MODE, 0x04);
    assert_eq!(POLL_INTERVAL, Duration::from_secs(10));
}
