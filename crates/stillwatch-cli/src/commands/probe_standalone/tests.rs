use jiff::Timestamp;
use jiff::tz::TimeZone;
use stillwatch_core::stats::{BlockState, Threshold, ThresholdReason};
use stillwatch_ipc::json::to_json;
use stillwatch_ipc::probe::{ProbeOutput, ProbeSample};

use super::*;
use crate::render;

fn fixture() -> ProbeSample {
    let blocks = vec![
        BlockState::Persistent,
        BlockState::Persistent,
        BlockState::Changed,
        BlockState::Dark,
    ];
    ProbeSample {
        at: Timestamp::constant(1_790_000_000, 0),
        threshold: Threshold::new(70, ThresholdReason::Normal),
        stale: false,
        outputs: vec![ProbeOutput::from_blocks("HDMI-A-1", 2, 2, blocks, 70).unwrap()],
    }
}

fn fixture_json() -> String {
    to_json(&fixture()).unwrap()
}

fn plain() -> Style {
    Style::plain(TimeZone::UTC)
}

#[test]
fn renderer_snapshot_matches_the_shared_grid() {
    let json = fixture_json();
    assert!(!json.contains("luma"), "{json}");
    let mut out = Vec::new();
    write_line(&json, false, &plain(), &mut out).unwrap();
    assert_eq!(
        String::from_utf8(out).unwrap(),
        render::probe::render(&fixture(), &plain())
    );
}

#[test]
fn json_forwards_the_line_without_reserializing() {
    let json = fixture_json();
    let mut out = Vec::new();
    write_line(&json, true, &plain(), &mut out).unwrap();
    assert_eq!(String::from_utf8(out).unwrap(), format!("{json}\n"));
}

#[test]
fn stillwatchd_argv_forwards_probe_flags() {
    let args = ProbeArgs {
        interval: Some(std::time::Duration::from_secs(5)),
        count: std::num::NonZeroUsize::new(3),
        json: true,
        standalone: true,
    };
    let forwarded = daemon_argv(&args, Some("debug"));
    let text: Vec<String> = forwarded
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        text,
        [
            "--probe",
            "--log-level",
            "debug",
            "--interval",
            "5s",
            "--count",
            "3"
        ]
    );
    assert_eq!(stillwatchd_path().file_name().unwrap(), "stillwatchd");
}

#[test]
fn a_bad_line_is_an_error() {
    let err = write_line("not-json", false, &plain(), &mut Vec::new()).unwrap_err();
    assert!(err.to_string().contains("JSON"), "{err}");
}
