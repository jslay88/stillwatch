use jiff::Timestamp;
use jiff::tz::TimeZone;
use stillwatch_core::state::State;
use stillwatch_core::stats::{BlockCounts, OutputStats, Threshold, ThresholdReason};

use super::*;

fn plain() -> Style {
    Style::plain(TimeZone::UTC)
}

fn detection() -> DetectionStats {
    let counts = BlockCounts {
        total: 100,
        counted: 82,
        persistent: 59,
        dark: 18,
        ignored: 0,
    };
    DetectionStats {
        outputs: vec![OutputStats::from_counts("HDMI-A-1", counts, 70)],
        threshold: Threshold::new(70, ThresholdReason::Normal),
        stale: true,
    }
}

#[test]
fn minimal_status_snapshot() {
    assert_eq!(
        render(&StatusPayload::new(State::Active), &plain()),
        "\
state       active for 0s
idle        no
locked      no
media       not playing
capture     none (input idle only)
last check  none yet
config      ok
"
    );
}

#[test]
fn full_status_snapshot() {
    let status = StatusPayload {
        state_seconds: 252,
        snooze_remaining_seconds: Some(2448),
        idle: true,
        locked: false,
        media_playing: true,
        capture_backend: Some("kwin".into()),
        last_detection: Some(detection()),
        config_errors: vec![
            "stale.stale_percent: must be between 1 and 100, got 0".into(),
            "action.command: must not be empty".into(),
        ],
        panel_care: Some(PanelCareStatus {
            screen_on_seconds: 3 * 3600,
            last_standby: Some(Timestamp::from_second(1_790_000_000).unwrap()),
            overlay_uses: 2,
        }),
        ..StatusPayload::new(State::Snoozed)
    };
    assert_eq!(
        render(&status, &plain()),
        "\
state       snoozed for 4m 12s
snooze      40m 48s left
idle        yes
locked      no
media       playing
capture     kwin
last check  STALE, threshold 70% normal
  HDMI-A-1: persistent 72% (dark 18%, counted 82%), threshold 70% normal -> STALE
panel care  screen on 3h, last standby 2026-09-21 14:13:20, overlay used 2 times
config      the last reload failed (2 problems); the last good config is still in effect
  stale.stale_percent: must be between 1 and 100, got 0
  action.command: must not be empty
"
    );
}

#[test]
fn singular_counts_and_never_standby() {
    let status = StatusPayload {
        locked: true,
        config_errors: vec!["bad".into()],
        panel_care: Some(PanelCareStatus {
            screen_on_seconds: 60,
            last_standby: None,
            overlay_uses: 1,
        }),
        last_detection: Some(DetectionStats {
            stale: false,
            ..detection()
        }),
        ..StatusPayload::new(State::Locked)
    };
    let text = render(&status, &plain());
    assert!(text.contains("\nlocked      yes\n"), "{text}");
    assert!(text.contains("last check  not stale, threshold 70% normal\n"));
    assert!(text.contains("screen on 1m, last standby never, overlay used 1 time\n"));
    assert!(text.contains("the last reload failed (1 problem);"));
}
