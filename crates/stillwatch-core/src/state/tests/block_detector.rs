//! The machine driven by a real [`BlockDetector`] through [`StaleDetector`].

use std::time::Duration;

use super::{changed, transition_record};
use crate::command::Command;
use crate::config::{Config, IgnoreRegion};
use crate::detector::BlockDetector;
use crate::event::{CaptureFrame, Event};
use crate::luma::{LumaGrid, OutputInfo};
use crate::mocks::Harness;
use crate::state::State;

fn watching(config: &Config) -> Harness {
    let mut h = Harness::with_config(config);
    h.replace_detector(Box::new(BlockDetector::new(config)));
    h.idle();
    assert_eq!(h.state(), State::Monitoring);
    h
}

fn interval(config: &Config) -> Duration {
    Duration::from_secs(u64::from(config.stale.check_interval_seconds))
}

/// Waits for the next capture request and completes it with a 64x36 grid
/// filled with `value` on `HDMI-A-1`.
fn capture(h: &mut Harness, config: &Config, value: u8) -> Vec<Command> {
    capture_grid(h, config, LumaGrid::filled(64, 36, value).unwrap())
}

/// Waits for the next capture request and completes it with `grid` on
/// `HDMI-A-1`.
fn capture_grid(h: &mut Harness, config: &Config, grid: LumaGrid) -> Vec<Command> {
    let requested = h.advance(interval(config));
    assert!(
        requested
            .iter()
            .any(|command| matches!(command, Command::RequestCapture { .. }))
    );
    let frames = vec![CaptureFrame {
        output: "HDMI-A-1".into(),
        grid,
    }];
    h.send(Event::CaptureCompleted { frames })
}

/// A 64x32 grid (two rows per block on the default 16x16 grid) whose top
/// half flips between two bright values every capture while the bottom half
/// holds still: 50% of the blocks can persist.
fn top_half_moving(capture_index: u32) -> LumaGrid {
    let flicker = if capture_index.is_multiple_of(2) {
        60
    } else {
        200
    };
    LumaGrid::from_fn(64, 32, |_, y| if y < 16 { flicker } else { 128 }).unwrap()
}

#[test]
fn a_constant_screen_prompts_after_persist_checks_unchanged_captures() {
    let config = Config::default();
    let mut h = watching(&config);
    for _ in 0..config.stale.persist_checks {
        capture(&mut h, &config, 128);
        assert_eq!(h.state(), State::Monitoring);
    }
    let commands = capture(&mut h, &config, 128);
    assert!(commands.contains(&changed(State::Monitoring, State::Prompting)));
    assert_eq!(h.state(), State::Prompting);
    let detection = transition_record(&commands).detection.unwrap();
    assert!(detection.stale);
    assert_eq!(detection.outputs[0].output, "HDMI-A-1");
    assert!((detection.outputs[0].persistent_percent - 100.0).abs() < f64::EPSILON);
}

#[test]
fn a_changing_screen_keeps_monitoring() {
    let config = Config::default();
    let mut h = watching(&config);
    for capture_index in 0..4 * config.stale.persist_checks {
        let value = if capture_index.is_multiple_of(2) {
            60
        } else {
            200
        };
        capture(&mut h, &config, value);
        assert_eq!(h.state(), State::Monitoring);
    }
}

#[test]
fn input_mid_watch_starts_the_next_watch_from_a_baseline() {
    let config = Config::default();
    let mut h = watching(&config);
    for _ in 0..config.stale.persist_checks {
        capture(&mut h, &config, 128);
    }
    h.input();
    h.idle();
    for _ in 0..config.stale.persist_checks {
        capture(&mut h, &config, 128);
        assert_eq!(h.state(), State::Monitoring);
    }
    capture(&mut h, &config, 128);
    assert_eq!(h.state(), State::Prompting);
}

#[test]
fn captures_that_do_not_fit_the_block_grid_never_prompt() {
    let config = Config::default();
    let mut h = watching(&config);
    for _ in 0..3 * config.stale.persist_checks {
        h.advance(interval(&config));
        h.complete_capture();
        assert_eq!(h.state(), State::Monitoring);
    }
}

#[test]
fn a_threshold_reload_keeps_the_counters_and_applies_on_the_next_capture() {
    let config = Config::default();
    let mut h = watching(&config);
    for capture_index in 0..2 * config.stale.persist_checks {
        capture_grid(&mut h, &config, top_half_moving(capture_index));
        assert_eq!(h.state(), State::Monitoring, "50% is under 70%");
    }

    let mut lower = config.clone();
    lower.stale.stale_percent = 40;
    h.apply_config(&lower);
    let index = 2 * config.stale.persist_checks;
    let commands = capture_grid(&mut h, &lower, top_half_moving(index));
    assert_eq!(h.state(), State::Prompting);
    let detection = transition_record(&commands).detection.unwrap();
    assert_eq!(detection.threshold.percent, 40);
    assert!((detection.outputs[0].persistent_percent - 50.0).abs() < f64::EPSILON);
}

#[test]
fn a_block_grid_reload_restarts_from_a_baseline() {
    let config = Config::default();
    let mut h = watching(&config);
    for _ in 0..config.stale.persist_checks {
        capture(&mut h, &config, 128);
    }
    let mut coarser = config.clone();
    coarser.stale.block_grid = [8, 8];
    h.apply_config(&coarser);
    for _ in 0..coarser.stale.persist_checks {
        capture(&mut h, &coarser, 128);
        assert_eq!(h.state(), State::Monitoring);
    }
    capture(&mut h, &coarser, 128);
    assert_eq!(h.state(), State::Prompting);
}

#[test]
fn ignore_regions_apply_once_output_sizes_arrive() {
    let mut config = Config::default();
    config.stale.ignore_regions = vec![IgnoreRegion {
        output: "HDMI-A-1".into(),
        x: 0,
        y: 0,
        w: 3840,
        h: 1080,
    }];
    let mut h = watching(&config);
    for capture_index in 0..2 * config.stale.persist_checks {
        capture_grid(&mut h, &config, top_half_moving(capture_index));
        assert_eq!(h.state(), State::Monitoring, "no size, nothing ignored");
    }

    let commands = h.send(Event::OutputsChanged(vec![OutputInfo::new(
        "HDMI-A-1", 3840, 2160,
    )]));
    assert_eq!(commands, vec![]);
    let index = 2 * config.stale.persist_checks;
    let commands = capture_grid(&mut h, &config, top_half_moving(index));
    assert_eq!(h.state(), State::Prompting);
    let detection = transition_record(&commands).detection.unwrap();
    assert!((detection.outputs[0].counted_percent - 50.0).abs() < f64::EPSILON);
    assert!((detection.outputs[0].persistent_percent - 100.0).abs() < f64::EPSILON);
}

#[test]
fn an_unplugged_output_loses_its_counters() {
    let config = Config::default();
    let mut h = watching(&config);
    let hdmi = OutputInfo::new("HDMI-A-1", 3840, 2160);
    h.send(Event::OutputsChanged(vec![hdmi.clone()]));
    for _ in 0..config.stale.persist_checks {
        capture(&mut h, &config, 128);
    }
    h.send(Event::OutputsChanged(vec![]));
    h.send(Event::OutputsChanged(vec![hdmi]));
    for _ in 0..config.stale.persist_checks {
        capture(&mut h, &config, 128);
        assert_eq!(h.state(), State::Monitoring);
    }
    capture(&mut h, &config, 128);
    assert_eq!(h.state(), State::Prompting);
}
