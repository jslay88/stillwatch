//! The machine driven by a real [`BlockDetector`] through [`StaleDetector`].

use std::time::Duration;

use super::{changed, transition_record};
use crate::command::Command;
use crate::config::Config;
use crate::detector::BlockDetector;
use crate::event::{CaptureFrame, Event};
use crate::luma::LumaGrid;
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
    let requested = h.advance(interval(config));
    assert!(
        requested
            .iter()
            .any(|command| matches!(command, Command::RequestCapture { .. }))
    );
    let frames = vec![CaptureFrame {
        output: "HDMI-A-1".into(),
        grid: LumaGrid::filled(64, 36, value).unwrap(),
    }];
    h.send(Event::CaptureCompleted { frames })
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
