use std::fs;
use std::sync::Arc;

use jiff::Timestamp;
use stillwatch_core::command::BlankMethod;
use stillwatch_core::history::{
    DecisionContext, HistoryEntry, HistoryKind, PromptMedium, PromptReason,
};
use stillwatch_core::state::State;
use stillwatch_core::stats::{
    BlockCounts, DetectionStats, OutputStats, Threshold, ThresholdReason,
};
use stillwatch_ipc::json::to_json_lines;
use stillwatch_ipc::proxy::StillwatchProxy;
use stillwatch_testkit::PrivateBus;
use stillwatchd::service::Service;
use stillwatchd::service::fake::FakeHandle;
use zbus::proxy::CacheProperties;

use super::detail::{accepts, detail_lines};
use super::load::{load, read_history};
use super::{
    HistMsg, HistoryLoad, HistoryPage, HistoryRow, HistorySource, KindFilter, TimeRange, apply,
};
use crate::model::update;
use crate::page::Page;
use crate::shell::{DaemonCall, Message, Shell, Visibility};

const NOW: i64 = 1_800_000_000;

fn at(second: i64) -> Timestamp {
    Timestamp::from_second(second).unwrap()
}

fn row(second: i64, kind: HistoryKind) -> HistoryRow {
    HistoryRow {
        at: second,
        kind,
        label: kind_name(kind),
        detail: Vec::new(),
    }
}

fn kind_name(kind: HistoryKind) -> String {
    super::detail::kind_label(kind).to_owned()
}

fn entry(second: i64, kind: HistoryKind) -> HistoryEntry {
    HistoryEntry::new(at(second), kind)
}

#[test]
fn range_and_kind_filters_keep_the_newest_first() {
    let rows = [
        row(NOW - 100, HistoryKind::Blank),
        row(NOW - 3_600, HistoryKind::Prompt),
        row(NOW - 3_601, HistoryKind::Snooze),
        row(NOW - 7 * 3_600, HistoryKind::Blank),
        row(NOW - 30 * 3_600, HistoryKind::Transition),
    ];
    let hour: Vec<i64> = rows
        .iter()
        .filter(|row| accepts(row, TimeRange::Hour, KindFilter::All, NOW))
        .map(|row| row.at)
        .collect();
    assert_eq!(hour, [NOW - 100, NOW - 3_600]);
    let six: Vec<_> = rows
        .iter()
        .filter(|row| {
            accepts(
                row,
                TimeRange::SixHours,
                KindFilter::Kind(HistoryKind::Blank),
                NOW,
            )
        })
        .map(|row| row.at)
        .collect();
    assert_eq!(six, [NOW - 100]);
    assert!(accepts(
        &rows[4],
        TimeRange::All,
        KindFilter::Kind(HistoryKind::Transition),
        NOW
    ));
    assert!(!accepts(&rows[4], TimeRange::Day, KindFilter::All, NOW));

    let mut page = HistoryPage {
        rows: rows.to_vec(),
        ..HistoryPage::default()
    };
    let shown: Vec<i64> = page.shown(NOW).iter().map(|row| row.at).collect();
    assert_eq!(
        shown,
        [NOW - 100, NOW - 3_600, NOW - 3_601, NOW - 7 * 3_600,]
    );
    page.kind = KindFilter::Kind(HistoryKind::Blank);
    assert_eq!(
        page.shown(NOW).iter().map(|row| row.at).collect::<Vec<_>>(),
        [NOW - 100, NOW - 7 * 3_600]
    );
}

#[test]
fn a_later_load_appends_and_drops_a_missing_selection() {
    let mut page = HistoryPage::default();
    assert_eq!(
        apply(
            &mut page,
            HistMsg::Loaded(HistoryLoad {
                rows: vec![row(NOW, HistoryKind::Blank)],
                source: HistorySource::Daemon,
            }),
        ),
        Vec::new()
    );
    let _ = apply(&mut page, HistMsg::Select(NOW));
    let _ = apply(
        &mut page,
        HistMsg::Loaded(HistoryLoad {
            rows: vec![
                row(NOW, HistoryKind::Blank),
                row(NOW + 5, HistoryKind::Snooze),
            ],
            source: HistorySource::Daemon,
        }),
    );
    assert_eq!(page.rows.len(), 2);
    assert_eq!(page.selected, Some(NOW));
    assert_eq!(page.shown(NOW + 10)[0].at, NOW + 5);

    let _ = apply(
        &mut page,
        HistMsg::Loaded(HistoryLoad {
            rows: vec![row(NOW + 5, HistoryKind::Snooze)],
            source: HistorySource::File,
        }),
    );
    assert_eq!(page.selected, None);
    assert_eq!(page.source, Some(HistorySource::File));
}

#[test]
fn detail_lists_percentages_threshold_and_context() {
    let counts = BlockCounts {
        total: 100,
        counted: 80,
        persistent: 60,
        dark: 18,
        ignored: 2,
    };
    let entry = HistoryEntry::transition(at(NOW), State::Monitoring, State::Blanked)
        .with_detection(DetectionStats {
            outputs: vec![
                OutputStats::from_counts("HDMI-A-1", counts, 70),
                OutputStats::from_counts("DP-1", counts, 70),
            ],
            threshold: Threshold::new(90, ThresholdReason::Media),
            stale: true,
        })
        .with_blank_method(BlankMethod::Dpms)
        .with_reblank_attempt(2)
        .with_prompt(PromptMedium::Dialog, PromptReason::Fullscreen)
        .with_context(DecisionContext {
            media_playing: true,
            gamepad_active: false,
            locked: true,
        });
    let lines = detail_lines(&entry);
    let text = lines.join("\n");
    assert!(
        text.contains("HDMI-A-1: persistent 75%, dark 18%, counted 80%"),
        "{text}"
    );
    assert!(
        text.contains("DP-1: persistent 75%, dark 18%, counted 80%"),
        "{text}"
    );
    assert!(text.contains("Threshold 90% (media)"), "{text}");
    assert!(text.contains("Media: playing"), "{text}");
    assert!(text.contains("Gamepad: idle"), "{text}");
    assert!(text.contains("Session: locked"), "{text}");
    assert!(text.contains("Re-blank attempt: 2"), "{text}");
    assert!(text.contains("Blank method: dpms"), "{text}");
    assert!(text.contains("Overlay: not used"), "{text}");
    assert!(text.contains("Prompt: dialog (fullscreen)"), "{text}");

    let overlay = entry.with_blank_method(BlankMethod::Overlay);
    assert!(
        detail_lines(&overlay)
            .iter()
            .any(|line| line == "Overlay: used")
    );
    let used = HistoryEntry::new(at(NOW), HistoryKind::OverlayUsed);
    assert!(
        detail_lines(&used)
            .iter()
            .any(|line| line == "Overlay: used")
    );
}

#[test]
fn changing_the_range_asks_for_that_window() {
    let mut shell = Shell::new(vec![15]);
    shell.settings = Visibility::Open;
    let calls = update(&mut shell, Message::Navigate(Page::History));
    assert_eq!(
        calls,
        vec![DaemonCall::LoadHistory {
            since_seconds: TimeRange::Day.since_seconds(),
        }]
    );
    let calls = update(
        &mut shell,
        Message::History(HistMsg::Range(TimeRange::Hour)),
    );
    assert_eq!(
        calls,
        vec![DaemonCall::LoadHistory {
            since_seconds: 3_600,
        }]
    );
    assert_eq!(
        update(
            &mut shell,
            Message::History(HistMsg::Kind(KindFilter::Kind(HistoryKind::Blank)))
        ),
        Vec::new()
    );
    assert_eq!(
        update(&mut shell, Message::PollPage),
        vec![DaemonCall::LoadHistory {
            since_seconds: 3_600,
        }]
    );
}

#[test]
fn the_file_reader_skips_a_torn_line_and_a_missing_file() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("history.jsonl");
    assert_eq!(read_history(&missing).unwrap(), Vec::new());

    let path = dir.path().join("state").join("history.jsonl");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let entries = vec![
        entry(NOW, HistoryKind::Blank),
        entry(NOW + 1, HistoryKind::Prompt),
    ];
    let mut text = to_json_lines(&entries).unwrap();
    text.push_str("not-json\n");
    fs::write(&path, text).unwrap();
    let read = read_history(&path).unwrap();
    assert_eq!(read, entries);
}

#[tokio::test]
async fn load_uses_the_daemon_when_it_is_up_and_the_file_when_it_is_not() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.jsonl");
    let stored = entry(NOW, HistoryKind::Snooze);
    fs::write(&path, to_json_lines(std::slice::from_ref(&stored)).unwrap()).unwrap();
    let from_file = load(None, 0, &path).await.unwrap();
    assert_eq!(from_file.source, HistorySource::File);
    assert_eq!(from_file.rows.len(), 1);
    assert_eq!(from_file.rows[0].kind, HistoryKind::Snooze);

    let Some(bus) = PrivateBus::start().unwrap() else {
        return;
    };
    let server = bus.connect().await.unwrap();
    let fake = Arc::new(FakeHandle::new());
    fake.update(|state| {
        state.history = vec![entry(NOW, HistoryKind::Blank)];
    });
    let _service = Service::claim(server, Arc::clone(&fake) as _)
        .await
        .unwrap();
    let client = bus.connect().await.unwrap();
    let proxy = StillwatchProxy::builder(&client)
        .cache_properties(CacheProperties::No)
        .build()
        .await
        .unwrap();
    let from_daemon = load(Some(&proxy), 0, &path).await.unwrap();
    assert_eq!(from_daemon.source, HistorySource::Daemon);
    assert_eq!(from_daemon.rows.len(), 1);
    assert_eq!(from_daemon.rows[0].kind, HistoryKind::Blank);
    assert_eq!(fake.state().history_since.len(), 1);
}

#[test]
fn every_kind_has_a_choice() {
    let kinds = [
        HistoryKind::Transition,
        HistoryKind::Prompt,
        HistoryKind::Snooze,
        HistoryKind::Blank,
        HistoryKind::Reblank,
        HistoryKind::Ceiling,
        HistoryKind::ConfigReload,
        HistoryKind::OverlayUsed,
        HistoryKind::PromptAnswered,
        HistoryKind::ConfigReloadFailed,
        HistoryKind::Migration,
        HistoryKind::Reconnect,
        HistoryKind::Hotplug,
    ];
    let choices = KindFilter::choices();
    assert_eq!(choices[0], KindFilter::All);
    for kind in kinds {
        assert!(choices.contains(&KindFilter::Kind(kind)), "{kind:?}");
        assert_ne!(kind_name(kind), "");
    }
}
