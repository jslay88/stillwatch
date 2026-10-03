use std::io;
use std::sync::{Arc, Mutex, PoisonError};

use super::*;
use crate::config::StaleRequire;
use crate::luma::LumaGrid;
use crate::stats::ThresholdReason;

const STILL: u8 = 128;

fn frame(output: &str, width: u32, height: u32, value: u8) -> CaptureFrame {
    CaptureFrame {
        output: output.into(),
        grid: LumaGrid::filled(width, height, value).unwrap(),
    }
}

/// A grid that fits the default 16x16 block grid.
fn fitting(output: &str, value: u8) -> CaptureFrame {
    frame(output, 64, 36, value)
}

/// Alternates between two bright values so every block changes each capture.
fn flicker(capture: u32) -> u8 {
    if capture.is_multiple_of(2) { 60 } else { 200 }
}

#[test]
fn a_constant_grid_goes_stale_on_the_same_capture_as_observe_means() {
    let config = Config::default();
    let mut from_frames = BlockDetector::new(&config);
    let mut from_means = BlockDetector::new(&config);
    let means = [f32::from(STILL); 256];
    let frames = [fitting("DP-1", STILL)];
    for capture in 0..=config.stale.persist_checks {
        let stats = from_frames.observe(&frames, &[]);
        let expected = from_means.observe_means(
            &[BlockMeans {
                output: "DP-1",
                means: &means,
            }],
            &[],
        );
        assert_eq!(stats, expected);
        assert_eq!(stats.stale, capture == config.stale.persist_checks);
    }
    assert_eq!(from_frames.blocks("DP-1"), from_means.blocks("DP-1"));
}

#[test]
fn a_changing_grid_never_goes_stale() {
    let mut detector = BlockDetector::new(&Config::default());
    for capture in 0..20 {
        let stats = detector.observe(&[fitting("DP-1", flicker(capture))], &[]);
        assert!(!stats.stale);
    }
}

#[test]
fn playing_players_pick_the_threshold() {
    let mut detector = BlockDetector::new(&Config::default());
    let stats = detector.observe(&[fitting("DP-1", STILL)], &["mpv".into()]);
    assert_eq!(stats.threshold.reason, ThresholdReason::Media);
}

#[test]
fn a_grid_smaller_than_the_block_grid_counts_nothing_and_resets_history() {
    let mut config = Config::default();
    config.stale.require = StaleRequire::All;
    config.safety.ceiling_minutes = 5;
    let mut detector = BlockDetector::new(&config);
    let good = [fitting("DP-1", STILL), fitting("DP-2", STILL)];
    for _ in 0..=config.stale.persist_checks {
        detector.observe(&good, &[]);
    }
    assert!(detector.observe(&good, &[]).stale);

    let stats = detector.observe(&[fitting("DP-1", STILL), frame("DP-2", 8, 8, STILL)], &[]);
    assert!(stats.outputs[0].stale);
    assert_eq!(stats.outputs[1].output, "DP-2");
    assert!(!stats.outputs[1].stale);
    assert!(stats.outputs[1].counted_percent.abs() < f64::EPSILON);
    assert!(!stats.stale);
    assert!(!detector.ceiling().unwrap().stale);
    assert_eq!(detector.blocks("DP-2"), None);

    for _ in 0..config.stale.persist_checks {
        assert!(!detector.observe(&good, &[]).stale);
    }
    assert!(detector.observe(&good, &[]).stale);
}

#[test]
fn only_grids_that_do_not_fit_is_not_stale() {
    let mut detector = BlockDetector::new(&Config::default());
    for _ in 0..10 {
        let stats = detector.observe(&[frame("DP-1", 15, 40, STILL)], &[]);
        assert!(!stats.stale);
        assert_eq!(stats.outputs.len(), 1);
    }
}

#[derive(Clone, Default)]
struct Logs(Arc<Mutex<Vec<u8>>>);

impl Logs {
    fn text(&self) -> String {
        let bytes = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

impl io::Write for Logs {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn logged(run: impl FnOnce()) -> String {
    let logs = Logs::default();
    let writer = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_ansi(false)
        .without_time()
        .finish();
    tracing::subscriber::with_default(subscriber, run);
    logs.text()
}

#[test]
fn a_skipped_grid_is_logged_with_the_output_and_dimensions_only() {
    let mut config = Config::default();
    config.stale.monitored_outputs = vec!["DP-1".into()];
    let mut detector = BlockDetector::new(&config);
    let text = logged(|| {
        detector.observe(
            &[frame("DP-1", 12, 9, 77), frame("HDMI-A-1", 4, 4, 77)],
            &[],
        );
    });
    assert_eq!(text.lines().count(), 1, "{text}");
    assert!(text.contains("WARN"), "{text}");
    assert!(text.contains("output=DP-1"), "{text}");
    assert!(text.contains("16x16 block grid"), "{text}");
    assert!(text.contains("12x9 luma grid"), "{text}");
    assert!(!text.contains("77"), "{text}");
    assert!(!text.contains("HDMI-A-1"), "{text}");
}

#[test]
fn fitting_grids_log_nothing() {
    let mut detector = BlockDetector::new(&Config::default());
    let text = logged(|| {
        detector.observe(&[fitting("DP-1", STILL)], &[]);
    });
    assert_eq!(text, "");
}

#[test]
fn the_trait_delegates_to_the_inherent_methods() {
    let mut config = Config::default();
    config.safety.ceiling_minutes = 5;
    let mut detector = BlockDetector::new(&config);
    let stale: &mut dyn StaleDetector = &mut detector;
    for _ in 0..config.stale.persist_checks {
        assert!(!stale.observe(&[fitting("DP-1", STILL)], &[]).stale);
    }
    assert!(stale.observe(&[fitting("DP-1", STILL)], &[]).stale);
    let ceiling = StaleDetector::ceiling(&detector);
    assert!(ceiling.as_ref().unwrap().stale);
    assert_eq!(ceiling, BlockDetector::ceiling(&detector));

    StaleDetector::reset(&mut detector);
    assert_eq!(detector.blocks("DP-1"), None);
    assert_eq!(
        StaleDetector::ceiling(&detector),
        BlockDetector::ceiling(&detector)
    );
}

#[test]
fn the_trait_ceiling_is_none_when_disabled() {
    let mut config = Config::default();
    config.safety.ceiling_enabled = false;
    let mut detector = BlockDetector::new(&config);
    StaleDetector::observe(&mut detector, &[fitting("DP-1", STILL)], &[]);
    assert_eq!(StaleDetector::ceiling(&detector), None);
    assert_eq!(BlockDetector::ceiling(&detector), None);
}
