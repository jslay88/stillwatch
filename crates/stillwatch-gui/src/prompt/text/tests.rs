use stillwatch_core::state::State;
use stillwatch_core::stats::{
    BlockCounts, DetectionStats, OutputStats, Threshold, ThresholdReason,
};
use stillwatch_ipc::status::StatusPayload;

use super::{countdown_line, parse_custom, preset_label, remaining_secs, static_summary};

fn output(name: &str, persistent: u32) -> OutputStats {
    OutputStats::from_counts(
        name,
        BlockCounts {
            total: 100,
            counted: 100,
            persistent,
            dark: 0,
            ignored: 0,
        },
        70,
    )
}

#[test]
fn summary_names_each_stale_output() {
    let status = StatusPayload {
        last_detection: Some(DetectionStats {
            outputs: vec![output("HDMI-A-1", 84), output("DP-1", 10)],
            threshold: Threshold::new(70, ThresholdReason::Normal),
            stale: true,
        }),
        ..StatusPayload::new(State::Prompting)
    };
    assert_eq!(static_summary(&status), "HDMI-A-1 is 84% unchanged");
    assert_eq!(
        static_summary(&StatusPayload::new(State::Prompting)),
        "The screen looks static."
    );
}

#[test]
fn countdown_and_preset_labels() {
    assert_eq!(countdown_line(0), "Blanking now");
    assert_eq!(countdown_line(12), "Blanking in 12s");
    assert_eq!(preset_label(15), "15 min");
    assert_eq!(preset_label(60), "1 hour");
    assert_eq!(preset_label(180), "3 hours");
    assert_eq!(preset_label(90), "90 min");
}

#[test]
fn remaining_uses_time_already_spent_prompting() {
    let mut status = StatusPayload::new(State::Prompting);
    status.state_seconds = 17;
    assert_eq!(remaining_secs(&status, 60), 43);
    status.state_seconds = 90;
    assert_eq!(remaining_secs(&status, 60), 0);
    status.state = State::Active;
    assert_eq!(remaining_secs(&status, 60), 60);
}

#[test]
fn custom_text_parses_humantime_and_bare_minutes() {
    assert_eq!(parse_custom("45m", 1, 720), Ok(45));
    assert_eq!(parse_custom("  45 ", 1, 720), Ok(45));
    assert_eq!(parse_custom("1h30m", 1, 720), Ok(90));
    assert_eq!(parse_custom("1h 30m", 1, 720), Ok(90));
    assert_eq!(parse_custom("2h", 1, 720), Ok(120));
    assert!(parse_custom("90s", 1, 720).is_err());
    assert!(parse_custom("0m", 1, 720).is_err());
    assert_eq!(parse_custom("721m", 1, 720).unwrap_err(), "at most 720 min");
    assert_eq!(parse_custom("1m", 5, 10).unwrap_err(), "at least 5 min");
    assert!(parse_custom("soon", 1, 720).unwrap_err().contains("45m"));
    assert!(parse_custom("", 1, 720).is_err());
}
