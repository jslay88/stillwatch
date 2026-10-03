use std::future;
use std::num::NonZeroUsize;
use std::time::Duration;

use stillwatch_core::backend::{BackendError, MediaPlayer};
use stillwatch_core::config::{Config, IgnoreRegion};
use stillwatch_core::detector::BlockDetector;
use stillwatch_core::luma::{LumaGrid, OutputInfo};
use stillwatch_core::mocks::MockCapture;
use stillwatch_core::stats::{BlockState, ThresholdReason};
use stillwatch_core::time::FakeClock;
use stillwatch_ipc::json::from_json_lines;
use stillwatch_ipc::probe::{MIN_PROBE_INTERVAL_MS, ProbeSample};

use super::{Settings, interval, load_config, run, sample};

const STILL: u8 = 128;
const HDMI: &str = "HDMI-A-1";

fn config() -> Config {
    let mut config = Config::default();
    config.stale.block_grid = [2, 2];
    config.stale.persist_checks = 2;
    config.stale.downscale_width = 8;
    config
}

fn grid() -> LumaGrid {
    LumaGrid::filled(8, 8, STILL).unwrap()
}

fn output() -> OutputInfo {
    OutputInfo::new(HDMI, 400, 400)
}

fn settings(count: usize) -> Settings {
    Settings {
        interval: Duration::from_millis(u64::from(MIN_PROBE_INTERVAL_MS)),
        count: NonZeroUsize::new(count),
        downscale_width: 8,
    }
}

fn queue(capture: &MockCapture, samples: usize) {
    capture.set_outputs(vec![output()]);
    for _ in 0..samples {
        capture.push_grid(grid());
    }
}

async fn collect(
    capture: &MockCapture,
    detector: &mut BlockDetector,
    clock: &FakeClock,
    settings: &Settings,
    playing: &[MediaPlayer],
) -> ProbeSample {
    sample(capture, detector, clock, settings.downscale_width, playing)
        .await
        .unwrap()
}

#[tokio::test(start_paused = true)]
async fn static_blocks_become_persistent_after_persist_checks() {
    let capture = MockCapture::new();
    let persist = config().stale.persist_checks;
    let count = persist as usize + 1;
    queue(&capture, count);
    let clock = FakeClock::new();
    let mut detector = BlockDetector::new(&config());
    let mut out = Vec::new();
    run(
        &capture,
        &mut detector,
        &clock,
        &settings(count),
        Vec::new,
        &mut out,
        future::pending(),
    )
    .await
    .unwrap();

    let json = String::from_utf8(out).unwrap();
    assert!(!json.contains("luma"), "{json}");
    let samples: Vec<ProbeSample> = from_json_lines(&json).unwrap();
    assert_eq!(samples.len(), count);
    assert_eq!(samples[0].outputs[0].width, 400);
    assert_eq!(samples[0].outputs[0].height, 400);
    assert!(
        samples[0].outputs[0]
            .blocks
            .iter()
            .all(|block| *block == BlockState::Changed)
    );
    let last = &samples[samples.len() - 1];
    assert!(
        last.outputs[0]
            .blocks
            .iter()
            .all(|block| *block == BlockState::Persistent)
    );
    assert!(last.stale);
    assert_eq!(last.threshold.reason, ThresholdReason::Normal);
    assert_eq!(capture.requests(), vec![(HDMI.into(), 8); count]);
}

#[tokio::test]
async fn ignore_regions_and_monitored_outputs() {
    let mut config = config();
    config.stale.ignore_regions = vec![IgnoreRegion {
        output: HDMI.into(),
        x: 0,
        y: 0,
        w: 100,
        h: 100,
    }];
    config.stale.monitored_outputs = vec![HDMI.into()];
    let capture = MockCapture::new();
    capture.set_outputs(vec![output(), OutputInfo::new("DP-1", 400, 400)]);
    capture.push_grid(grid());
    let clock = FakeClock::new();
    let mut detector = BlockDetector::new(&config);
    let taken = collect(&capture, &mut detector, &clock, &settings(1), &[]).await;
    assert_eq!(taken.outputs.len(), 1);
    assert_eq!(taken.outputs[0].stats.output, HDMI);
    assert_eq!(
        taken.outputs[0].blocks,
        [
            BlockState::Ignored,
            BlockState::Changed,
            BlockState::Changed,
            BlockState::Changed
        ]
    );
    assert_eq!(capture.requests(), [(HDMI.into(), 8)]);
}

#[tokio::test]
async fn playing_media_selects_the_media_threshold() {
    let capture = MockCapture::new();
    queue(&capture, 1);
    let clock = FakeClock::new();
    let mut detector = BlockDetector::new(&config());
    let taken = collect(
        &capture,
        &mut detector,
        &clock,
        &settings(1),
        &["mpv".into()],
    )
    .await;
    assert_eq!(taken.threshold.reason, ThresholdReason::Media);
    assert_eq!(taken.threshold.percent, 90);
}

#[tokio::test]
async fn a_capture_error_stops_the_loop() {
    let capture = MockCapture::new();
    capture.set_outputs(vec![output()]);
    capture.push_error(BackendError::PermissionDenied("kwin".into()));
    let clock = FakeClock::new();
    let mut detector = BlockDetector::new(&config());
    let err = run(
        &capture,
        &mut detector,
        &clock,
        &settings(2),
        Vec::new,
        &mut Vec::new(),
        future::pending(),
    )
    .await
    .unwrap_err();
    assert!(format!("{err:#}").contains("permission denied"), "{err:#}");
}

#[tokio::test(start_paused = true)]
async fn stop_ends_between_samples() {
    let capture = MockCapture::new();
    queue(&capture, 4);
    let clock = FakeClock::new();
    let mut detector = BlockDetector::new(&config());
    let mut out = Vec::new();
    let mut settings = settings(8);
    settings.count = None;
    run(
        &capture,
        &mut detector,
        &clock,
        &settings,
        Vec::new,
        &mut out,
        async {
            tokio::time::sleep(Duration::from_millis(150)).await;
        },
    )
    .await
    .unwrap();
    let samples: Vec<ProbeSample> = from_json_lines(&String::from_utf8(out).unwrap()).unwrap();
    assert_eq!(samples.len(), 2);
}

#[test]
fn missing_config_uses_defaults_and_a_bad_file_fails() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("nope.toml");
    assert_eq!(load_config(&missing).unwrap(), Config::default());
    let bad = dir.path().join("bad.toml");
    std::fs::write(&bad, "version = \"nope\"\n").unwrap();
    assert!(
        load_config(&bad)
            .unwrap_err()
            .to_string()
            .contains("config")
    );
}

#[test]
fn interval_comes_from_the_config_or_the_override() {
    let stale = Config::default().stale;
    assert_eq!(
        interval(None, &stale).unwrap(),
        Duration::from_secs(u64::from(stale.check_interval_seconds))
    );
    assert_eq!(
        interval(Some(Duration::from_secs(5)), &stale).unwrap(),
        Duration::from_secs(5)
    );
    let err = interval(Some(Duration::from_millis(50)), &stale).unwrap_err();
    assert!(
        err.to_string()
            .contains(&format!("at least {MIN_PROBE_INTERVAL_MS} ms")),
        "{err}"
    );
}
