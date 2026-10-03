use std::time::Duration;

use jiff::Timestamp;
use jiff::tz::TimeZone;
use stillwatch_core::history::{DecisionContext, PromptMedium, PromptReason};
use stillwatch_core::state::State;
use stillwatch_core::stats::{BlockCounts, OutputStats, Threshold, ThresholdReason};

use super::*;

fn at(offset: i64) -> Timestamp {
    Timestamp::from_second(1_790_000_000 + offset).unwrap()
}

fn plain() -> Style {
    Style::plain(TimeZone::UTC)
}

fn stats(output: &str, persistent: u32) -> OutputStats {
    let counts = BlockCounts {
        total: 100,
        counted: 100,
        persistent,
        dark: 0,
        ignored: 0,
    };
    OutputStats::from_counts(output, counts, 70)
}

fn stale(outputs: Vec<OutputStats>, reason: ThresholdReason) -> DetectionStats {
    DetectionStats {
        outputs,
        threshold: Threshold::new(70, reason),
        stale: true,
    }
}

#[test]
fn table_snapshot() {
    let entries = vec![
        HistoryEntry::transition(at(0), State::Monitoring, State::Prompting)
            .with_detection(stale(vec![stats("HDMI-A-1", 72)], ThresholdReason::Normal))
            .with_context(DecisionContext {
                media_playing: true,
                gamepad_active: false,
                locked: false,
            }),
        HistoryEntry::new(at(60), HistoryKind::PromptAnswered)
            .with_answer(PromptAnswer::Snooze)
            .with_snooze(Duration::from_mins(45)),
        HistoryEntry::new(at(120), HistoryKind::Blank)
            .with_blank_method(BlankMethod::DdcStandby)
            .with_context(DecisionContext {
                media_playing: false,
                gamepad_active: true,
                locked: true,
            }),
    ];
    assert_eq!(
        render(&entries, &plain()),
        "\
TIME                 EVENT            STATES                   DETECTION                            DETAIL           CONTEXT
2026-09-21 14:13:20  transition       monitoring -> prompting  HDMI-A-1 72% (threshold 70% normal)  -                media
2026-09-21 14:14:20  prompt answered  -                        -                                    snooze, 45m      -
2026-09-21 14:15:20  blank            -                        -                                    via ddc standby  gamepad, locked
"
    );
}

#[test]
fn empty_history_says_so() {
    assert_eq!(render(&[], &plain()), "no history entries\n");
}

#[test]
fn every_kind_has_a_label() {
    let kinds = [
        (HistoryKind::Transition, "transition"),
        (HistoryKind::Prompt, "prompt shown"),
        (HistoryKind::Snooze, "snooze"),
        (HistoryKind::Blank, "blank"),
        (HistoryKind::Reblank, "re-blank"),
        (HistoryKind::Ceiling, "ceiling"),
        (HistoryKind::ConfigReload, "config reload"),
        (HistoryKind::OverlayUsed, "overlay used"),
        (HistoryKind::PromptAnswered, "prompt answered"),
        (HistoryKind::ConfigReloadFailed, "reload failed"),
        (HistoryKind::Migration, "config migrated"),
        (HistoryKind::Reconnect, "reconnect"),
        (HistoryKind::Hotplug, "hotplug"),
    ];
    for (kind, label) in kinds {
        assert_eq!(event(kind), label);
    }
}

#[test]
fn details_cover_every_optional_field() {
    let answers = [
        (PromptAnswer::Custom, "custom"),
        (PromptAnswer::Cancel, "cancel"),
        (PromptAnswer::Timeout, "timeout"),
        (PromptAnswer::Dismissed, "dismissed"),
        (PromptAnswer::Failed, "failed"),
    ];
    for (answer, name) in answers {
        let entry = HistoryEntry::new(at(0), HistoryKind::PromptAnswered).with_answer(answer);
        assert_eq!(details(&entry), [name]);
    }
    let reblank = HistoryEntry::new(at(0), HistoryKind::Reblank)
        .with_blank_method(BlankMethod::Overlay)
        .with_reblank_attempt(2);
    assert_eq!(details(&reblank), ["via overlay", "attempt 2"]);
    let dpms = HistoryEntry::new(at(0), HistoryKind::Blank).with_blank_method(BlankMethod::Dpms);
    assert_eq!(details(&dpms), ["via dpms"]);
    let failed = HistoryEntry::new(at(0), HistoryKind::ConfigReloadFailed).with_error_count(1);
    assert_eq!(details(&failed), ["1 problem"]);
    let failed = failed.with_error_count(3);
    assert_eq!(details(&failed), ["3 problems"]);
    let migrated = HistoryEntry::new(at(0), HistoryKind::Migration).with_versions(1, 2);
    assert_eq!(details(&migrated), ["v1 -> v2"]);
    let shown = HistoryEntry::new(at(0), HistoryKind::Prompt)
        .with_prompt(PromptMedium::Dialog, PromptReason::FallbackClosed);
    assert_eq!(details(&shown), ["dialog", "closed without an action"]);
    let auto = HistoryEntry::new(at(0), HistoryKind::Prompt)
        .with_prompt(PromptMedium::Notification, PromptReason::Auto);
    assert_eq!(details(&auto), ["notification", "auto"]);
    let reconnect = HistoryEntry::new(at(0), HistoryKind::Reconnect).with_count(2);
    assert_eq!(details(&reconnect), ["2"]);
    let hotplug = HistoryEntry::new(at(0), HistoryKind::Hotplug)
        .with_output("HDMI-A-1")
        .with_count(1);
    assert_eq!(details(&hotplug), ["HDMI-A-1", "1"]);
}

#[test]
fn detection_lists_outputs_then_the_threshold() {
    let both = stale(
        vec![stats("HDMI-A-1", 72), stats("DP-1", 40)],
        ThresholdReason::Ceiling,
    );
    assert_eq!(
        detection(&both),
        "HDMI-A-1 72%, DP-1 40% (threshold 70% ceiling)"
    );
    let none = stale(Vec::new(), ThresholdReason::Media);
    assert_eq!(detection(&none), "threshold 70% media");
}

#[test]
fn half_transitions_name_their_side() {
    let mut entry = HistoryEntry::new(at(0), HistoryKind::Ceiling);
    entry.from = Some(State::Snoozed);
    assert_eq!(states(&entry), "from snoozed");
    entry.from = None;
    entry.to = Some(State::Prompting);
    assert_eq!(states(&entry), "to prompting");
}
