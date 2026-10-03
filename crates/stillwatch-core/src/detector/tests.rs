use super::*;
use crate::config::{CaptureBackend, IgnoreRegion, StaleRequire};
use crate::stats::{Threshold, ThresholdReason};

const STILL: f32 = 128.0;
const BLACK: f32 = 0.0;

fn config(cols: u32, rows: u32) -> Config {
    let mut config = Config::default();
    config.stale.block_grid = [cols, rows];
    config
}

fn detector(cols: u32, rows: u32) -> BlockDetector {
    BlockDetector::new(&config(cols, rows))
}

/// A capture where blocks matching `moving` flip between two bright values
/// every capture and the rest hold `still`.
fn frame(blocks: usize, capture: usize, still: f32, moving: impl Fn(usize) -> bool) -> Vec<f32> {
    let flicker = if capture.is_multiple_of(2) {
        60.0
    } else {
        200.0
    };
    (0..blocks)
        .map(|block| if moving(block) { flicker } else { still })
        .collect()
}

fn feed(
    detector: &mut BlockDetector,
    frames: &[(&str, &[f32])],
    playing: &[String],
) -> DetectionStats {
    let means: Vec<BlockMeans<'_>> = frames
        .iter()
        .map(|&(output, means)| BlockMeans { output, means })
        .collect();
    detector.observe_means(&means, playing)
}

/// Feeds enough captures for unchanged blocks to become persistent
/// (a baseline plus `persist_checks`) and returns the last result.
fn settle(
    detector: &mut BlockDetector,
    still: f32,
    moving: impl Fn(usize) -> bool + Copy,
    playing: &[String],
) -> DetectionStats {
    let blocks = detector.block_count();
    let mut last = None;
    for capture in 0..=detector.stale.persist_checks as usize {
        let means = frame(blocks, capture, still, moving);
        last = Some(feed(detector, &[("DP-1", &means)], playing));
    }
    last.unwrap()
}

fn assert_percent(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
}

fn players(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn constant_image_becomes_stale_after_persist_checks_unchanged_captures() {
    let mut detector = detector(4, 4);
    let means = [STILL; 16];
    for _ in 0..5 {
        assert!(!feed(&mut detector, &[("DP-1", &means)], &[]).stale);
    }
    let stats = feed(&mut detector, &[("DP-1", &means)], &[]);
    assert!(stats.stale);
    assert_percent(stats.outputs[0].persistent_percent, 100.0);
}

#[test]
fn a_toast_only_resets_the_blocks_it_touched() {
    let mut detector = detector(4, 4);
    settle(&mut detector, STILL, |_| false, &[]);
    let mut toast = [STILL; 16];
    toast[15] = 240.0;
    let stats = feed(&mut detector, &[("DP-1", &toast)], &[]);
    let blocks = detector.blocks("DP-1").unwrap();
    assert_eq!(blocks[15], BlockState::Changed);
    assert!(
        blocks[..15]
            .iter()
            .all(|&state| state == BlockState::Persistent)
    );
    assert_percent(stats.outputs[0].persistent_percent, 93.75);
    assert!(stats.stale);
}

#[test]
fn video_tiles_covering_30_percent_still_trigger_at_70() {
    let mut detector = detector(10, 10);
    let stats = settle(&mut detector, STILL, |block| block % 10 < 3, &[]);
    assert_eq!(stats.threshold, Threshold::new(70, ThresholdReason::Normal));
    assert_percent(stats.outputs[0].persistent_percent, 70.0);
    assert!(stats.stale);
}

#[test]
fn letterboxing_does_not_trigger() {
    let letterbox = |block: usize| (40..70).contains(&block);
    let mut detector = detector(10, 10);
    let stats = settle(&mut detector, BLACK, letterbox, &[]);
    assert!(!stats.stale);
    assert_percent(stats.outputs[0].dark_percent, 70.0);
    assert_percent(stats.outputs[0].counted_percent, 30.0);
    assert_percent(stats.outputs[0].persistent_percent, 0.0);

    let mut no_dark = config(10, 10);
    no_dark.stale.ignore_dark_below = 0;
    let mut detector = BlockDetector::new(&no_dark);
    assert!(settle(&mut detector, BLACK, letterbox, &[]).stale);
}

#[test]
fn an_all_dark_screen_is_not_stale() {
    let mut config = config(4, 4);
    config.stale.require = StaleRequire::Any;
    let mut detector = BlockDetector::new(&config);
    let stats = settle(&mut detector, BLACK, |_| false, &[]);
    assert!(!stats.stale);
    assert_percent(stats.outputs[0].dark_percent, 100.0);
    assert_percent(stats.outputs[0].counted_percent, 0.0);
    assert!(!detector.ceiling().unwrap().stale);
}

#[test]
fn ignore_regions_straddling_block_boundaries_are_skipped() {
    let mut config = config(4, 4);
    config.stale.ignore_regions = vec![IgnoreRegion {
        output: "DP-1".into(),
        x: 0,
        y: 0,
        w: 400,
        h: 101,
    }];
    let top_half_moves = |block: usize| block < 8;

    let mut unknown_size = BlockDetector::new(&config);
    assert!(!settle(&mut unknown_size, STILL, top_half_moves, &[]).stale);

    let mut detector = BlockDetector::new(&config);
    detector.set_outputs(&[OutputInfo::new("DP-1", 400, 400)]);
    let stats = settle(&mut detector, STILL, top_half_moves, &[]);
    assert!(stats.stale);
    assert_percent(stats.outputs[0].counted_percent, 50.0);
    let blocks = detector.blocks("DP-1").unwrap();
    assert!(
        blocks[..8]
            .iter()
            .all(|&state| state == BlockState::Ignored)
    );
    assert!(
        blocks[8..]
            .iter()
            .all(|&state| state == BlockState::Persistent)
    );
}

#[test]
fn stale_percent_boundaries() {
    for (moving, stale) in [(31, false), (30, true), (29, true)] {
        let mut detector = detector(10, 10);
        let stats = settle(&mut detector, STILL, |block| block < moving, &[]);
        assert_eq!(stats.stale, stale, "{} persistent", 100 - moving);
        assert_eq!(stats.threshold.reason, ThresholdReason::Normal);
    }
}

#[test]
fn media_stale_percent_boundaries() {
    let playing = players(&["mpv"]);
    for (moving, stale) in [(11, false), (10, true), (9, true)] {
        let mut detector = detector(10, 10);
        let stats = settle(&mut detector, STILL, |block| block < moving, &playing);
        assert_eq!(stats.stale, stale, "{} persistent", 100 - moving);
        assert_eq!(stats.threshold, Threshold::new(90, ThresholdReason::Media));
    }
}

#[test]
fn media_threshold_needs_a_non_ignored_player_and_a_nonzero_percent() {
    let eighty_percent = |block: usize| block < 20;
    let mut ignored_only = detector(10, 10);
    let stats = settle(
        &mut ignored_only,
        STILL,
        eighty_percent,
        &players(&["Spotify"]),
    );
    assert_eq!(stats.threshold.reason, ThresholdReason::Normal);
    assert!(stats.stale);

    let mut with_video = detector(10, 10);
    let stats = settle(
        &mut with_video,
        STILL,
        eighty_percent,
        &players(&["spotify", "mpv"]),
    );
    assert_eq!(stats.threshold.reason, ThresholdReason::Media);
    assert!(!stats.stale);

    let mut disabled = config(10, 10);
    disabled.stale.media_stale_percent = 0;
    let mut detector = BlockDetector::new(&disabled);
    let stats = settle(&mut detector, STILL, eighty_percent, &players(&["mpv"]));
    assert_eq!(stats.threshold.reason, ThresholdReason::Normal);
    assert!(stats.stale);
}

#[test]
fn the_same_75_percent_grid_is_stale_only_under_the_normal_threshold() {
    let seventy_five_percent = |block: usize| block < 25;
    let mut normal = detector(10, 10);
    let stats = settle(&mut normal, STILL, seventy_five_percent, &[]);
    assert_percent(stats.outputs[0].persistent_percent, 75.0);
    assert_eq!(stats.threshold, Threshold::new(70, ThresholdReason::Normal));
    assert!(stats.stale);

    let mut media = detector(10, 10);
    let stats = settle(&mut media, STILL, seventy_five_percent, &players(&["mpv"]));
    assert_percent(stats.outputs[0].persistent_percent, 75.0);
    assert_eq!(stats.threshold, Threshold::new(90, ThresholdReason::Media));
    assert!(!stats.stale);
}
fn two_outputs(require: StaleRequire) -> DetectionStats {
    let mut config = config(2, 2);
    config.stale.require = require;
    config.stale.persist_checks = 1;
    let mut detector = BlockDetector::new(&config);
    let still = [STILL; 4];
    feed(&mut detector, &[("DP-1", &still), ("DP-2", &[1.0; 4])], &[]);
    feed(
        &mut detector,
        &[("DP-1", &still), ("DP-2", &[250.0; 4])],
        &[],
    )
}

#[test]
fn require_all_needs_both_outputs_and_any_needs_one() {
    let all = two_outputs(StaleRequire::All);
    assert!(all.outputs[0].stale);
    assert!(!all.outputs[1].stale);
    assert!(!all.stale);
    assert!(two_outputs(StaleRequire::Any).stale);
    assert_eq!(all.outputs[0].output, "DP-1");
    assert_eq!(all.outputs[1].output, "DP-2");
}

#[test]
fn ceiling_needs_ceiling_minutes_of_unchanged_captures() {
    let mut config = config(4, 4);
    config.safety.ceiling_minutes = 10;
    let mut detector = BlockDetector::new(&config);
    let means = [STILL; 16];
    for _ in 0..10 {
        feed(&mut detector, &[("DP-1", &means)], &[]);
    }
    let ceiling = detector.ceiling().unwrap();
    assert_eq!(
        ceiling.threshold,
        Threshold::new(98, ThresholdReason::Ceiling)
    );
    assert!(!ceiling.stale);
    feed(&mut detector, &[("DP-1", &means)], &[]);
    let ceiling = detector.ceiling().unwrap();
    assert!(ceiling.stale);
    assert_percent(ceiling.outputs[0].persistent_percent, 100.0);
}

#[test]
fn ceiling_compares_against_ceiling_stale_percent() {
    let mut config = config(4, 4);
    config.safety.ceiling_minutes = 6;
    let mut detector = BlockDetector::new(&config);
    for capture in 0..8 {
        let means = frame(16, capture, STILL, |block| block == 0);
        feed(&mut detector, &[("DP-1", &means)], &[]);
    }
    let ceiling = detector.ceiling().unwrap();
    assert_percent(ceiling.outputs[0].persistent_percent, 93.75);
    assert!(!ceiling.stale);
}

#[test]
fn ceiling_is_none_when_disabled_and_empty_before_captures() {
    let mut config = config(4, 4);
    let ceiling = BlockDetector::new(&config).ceiling().unwrap();
    assert_eq!(ceiling.outputs, Vec::new());
    assert!(!ceiling.stale);
    config.safety.ceiling_enabled = false;
    assert_eq!(BlockDetector::new(&config).ceiling(), None);
}

#[test]
fn unmonitored_outputs_are_skipped() {
    let mut config = config(2, 2);
    config.stale.monitored_outputs = vec!["DP-2".into()];
    let mut detector = BlockDetector::new(&config);
    assert!(detector.monitors("DP-2"));
    assert!(!detector.monitors("DP-1"));
    let stats = feed(
        &mut detector,
        &[("DP-1", &[STILL; 4]), ("DP-2", &[STILL; 4])],
        &[],
    );
    assert_eq!(stats.outputs.len(), 1);
    assert_eq!(stats.outputs[0].output, "DP-2");
    assert_eq!(detector.blocks("DP-1"), None);
}

#[test]
fn means_that_do_not_match_the_grid_reset_the_output_and_count_nothing() {
    let mut detector = detector(2, 2);
    persisted(&mut detector);
    let stats = feed(&mut detector, &[("DP-1", &[STILL; 3])], &[]);
    assert_eq!(stats.outputs.len(), 1);
    assert_percent(stats.outputs[0].counted_percent, 0.0);
    assert!(!stats.stale);
    assert!(!detector.ceiling().unwrap().stale);
    assert_eq!(detector.blocks("DP-1"), None);

    let stats = feed(&mut detector, &[("DP-1", &[STILL; 4])], &[]);
    assert_eq!(detector.blocks("DP-1").unwrap(), &[BlockState::Changed; 4]);
    assert!(!stats.stale);
}

#[test]
fn a_mismatched_output_blocks_require_all_but_not_any() {
    for (require, stale) in [(StaleRequire::All, false), (StaleRequire::Any, true)] {
        let mut config = config(2, 2);
        config.stale.require = require;
        config.stale.persist_checks = 1;
        config.safety.ceiling_minutes = 1;
        let mut detector = BlockDetector::new(&config);
        let frames: [(&str, &[f32]); 2] = [("DP-1", &[STILL; 4]), ("DP-2", &[STILL; 3])];
        feed(&mut detector, &frames, &[]);
        let stats = feed(&mut detector, &frames, &[]);
        assert!(stats.outputs[0].stale);
        assert!(!stats.outputs[1].stale);
        assert_eq!(stats.stale, stale, "{require:?}");
        assert_eq!(detector.ceiling().unwrap().stale, stale, "{require:?}");
    }
}

fn persisted(detector: &mut BlockDetector) {
    settle(detector, STILL, |_| false, &[]);
    assert!(detector.blocks("DP-1").is_some());
}

#[test]
fn reset_and_drop_output_forget_history() {
    let mut detector = detector(2, 2);
    persisted(&mut detector);
    detector.reset();
    assert_eq!(detector.blocks("DP-1"), None);
    persisted(&mut detector);
    detector.drop_output("DP-1");
    assert_eq!(detector.blocks("DP-1"), None);
}

#[test]
fn a_reused_connector_name_starts_fresh() {
    let mut detector = detector(2, 2);
    persisted(&mut detector);
    detector.set_outputs(&[OutputInfo::new("DP-1", 100, 100)]);
    assert!(detector.blocks("DP-1").is_some());
    detector.set_outputs(&[OutputInfo::new("DP-1", 100, 100).with_generation(1)]);
    assert_eq!(detector.blocks("DP-1"), None);
    let stats = feed(&mut detector, &[("DP-1", &[STILL; 4])], &[]);
    assert!(!stats.stale);
    assert_eq!(detector.blocks("DP-1").unwrap(), &[BlockState::Changed; 4]);
}

#[test]
fn a_removed_output_drops_out_of_require_all() {
    let mut config = config(2, 2);
    config.stale.require = StaleRequire::All;
    config.stale.persist_checks = 1;
    let mut detector = BlockDetector::new(&config);
    let frames: [(&str, &[f32]); 2] = [("DP-1", &[STILL; 4]), ("DP-2", &[STILL; 3])];
    feed(&mut detector, &frames, &[]);
    let stats = feed(&mut detector, &frames, &[]);
    assert!(stats.outputs[0].stale);
    assert!(!stats.outputs[1].stale);
    assert!(!stats.stale, "require=all waits on every connected output");

    detector.set_outputs(&[OutputInfo::new("DP-1", 100, 100)]);
    assert_eq!(detector.blocks("DP-2"), None);
    let only = feed(&mut detector, &[("DP-1", &[STILL; 4])], &[]);
    assert!(only.stale);
    assert_eq!(only.outputs.len(), 1);
}

#[test]
fn set_outputs_drops_disconnected_outputs() {
    let mut detector = detector(2, 2);
    persisted(&mut detector);
    detector.set_outputs(&[OutputInfo::new("DP-1", 100, 100)]);
    assert!(detector.blocks("DP-1").is_some());
    detector.set_outputs(&[OutputInfo::new("DP-2", 100, 100)]);
    assert_eq!(detector.blocks("DP-1"), None);
}

#[test]
fn apply_config_resets_on_a_capture_backend_change() {
    let mut config = config(2, 2);
    let mut detector = BlockDetector::new(&config);
    persisted(&mut detector);
    detector.apply_config(&config);
    assert!(detector.blocks("DP-1").is_some());

    config.capture.backend = CaptureBackend::Portal;
    detector.apply_config(&config);
    assert_eq!(detector.blocks("DP-1"), None);
    persisted(&mut detector);
    detector.apply_config(&config);
    assert!(detector.blocks("DP-1").is_some());
}

#[test]
fn trait_apply_config_and_set_outputs_reach_the_detector() {
    let mut regridded = detector(2, 2);
    let mut unplugged = detector(2, 2);
    persisted(&mut regridded);
    persisted(&mut unplugged);

    StaleDetector::apply_config(&mut regridded, &config(4, 4));
    assert_eq!(regridded.grid(), [4, 4]);
    assert_eq!(regridded.blocks("DP-1"), None);

    StaleDetector::set_outputs(&mut unplugged, &[OutputInfo::new("DP-2", 100, 100)]);
    assert_eq!(unplugged.blocks("DP-1"), None);
}

#[test]
fn apply_config_resets_only_on_grid_or_monitored_output_changes() {
    let mut config = config(2, 2);
    config.stale.monitored_outputs = vec!["DP-1".into(), "DP-2".into()];
    let mut detector = BlockDetector::new(&config);
    persisted(&mut detector);

    config.stale.stale_percent = 50;
    config.stale.monitored_outputs = vec!["DP-2".into(), "DP-1".into()];
    detector.apply_config(&config);
    assert!(detector.blocks("DP-1").is_some());

    config.stale.monitored_outputs.push("HDMI-A-1".into());
    detector.apply_config(&config);
    assert_eq!(detector.blocks("DP-1"), None);

    persisted(&mut detector);
    config.stale.block_grid = [4, 4];
    detector.apply_config(&config);
    assert_eq!(detector.blocks("DP-1"), None);
    assert_eq!(detector.grid(), [4, 4]);
}

#[test]
fn apply_config_remaps_ignore_regions_without_a_reset() {
    let mut config = config(2, 2);
    let mut detector = BlockDetector::new(&config);
    detector.set_outputs(&[OutputInfo::new("DP-1", 100, 100)]);
    persisted(&mut detector);
    config.stale.ignore_regions = vec![IgnoreRegion {
        output: "DP-1".into(),
        x: 0,
        y: 0,
        w: 10,
        h: 10,
    }];
    detector.apply_config(&config);
    feed(&mut detector, &[("DP-1", &[STILL; 4])], &[]);
    assert_eq!(
        detector.blocks("DP-1").unwrap(),
        &[
            BlockState::Ignored,
            BlockState::Persistent,
            BlockState::Persistent,
            BlockState::Persistent
        ]
    );
}
