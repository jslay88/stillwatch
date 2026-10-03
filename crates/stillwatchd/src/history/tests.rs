use std::io::Write as _;
use std::path::Path;
use std::sync::{Arc, Mutex};

use stillwatch_core::history::HistoryKind;
use stillwatch_core::state::State;
use tempfile::TempDir;

use super::ring::{margin, parse};
use super::*;

fn at(second: i64) -> Timestamp {
    Timestamp::from_second(1_790_000_000 + second).unwrap()
}

fn entry(second: i64) -> HistoryEntry {
    HistoryEntry::transition(at(second), State::Active, State::Monitoring)
}

fn config(enabled: bool, max_entries: u32) -> HistoryConfig {
    HistoryConfig {
        enabled,
        max_entries,
    }
}

fn ring_in(dir: &TempDir, max_entries: u32) -> (HistoryRing, PathBuf) {
    let path = dir.path().join("state/stillwatch").join(FILE_NAME);
    (HistoryRing::new(&path, &config(true, max_entries)), path)
}

fn line_count(path: &Path) -> usize {
    std::fs::read_to_string(path).unwrap().lines().count()
}

async fn record_range(ring: &HistoryRing, seconds: std::ops::Range<i64>) {
    for second in seconds {
        ring.record(entry(second)).await.unwrap();
    }
}

async fn seconds(ring: &HistoryRing) -> Vec<i64> {
    ring.read(Timestamp::UNIX_EPOCH)
        .await
        .unwrap()
        .iter()
        .map(|entry| entry.at.as_second() - 1_790_000_000)
        .collect()
}

#[test]
fn margin_is_ten_percent_and_at_least_one() {
    assert_eq!(margin(1000), 100);
    assert_eq!(margin(25), 2);
    assert_eq!(margin(5), 1);
    assert_eq!(margin(1), 1);
    assert_eq!(margin(usize::MAX), usize::MAX / 100);
}

#[test]
fn default_path_is_in_the_state_dir() {
    let expected = paths::state_dir().map(|dir| dir.join("history.jsonl"));
    assert_eq!(default_path(), expected);
    if let Ok(ring) = HistoryRing::open_default(&HistoryConfig::default()) {
        assert_eq!(Ok(ring.path()), expected);
    }
}

#[tokio::test]
async fn appends_create_the_directory_and_read_back_in_order() {
    let dir = TempDir::new().unwrap();
    let (ring, path) = ring_in(&dir, 100);
    assert_eq!(ring.path(), path);
    assert_eq!(seconds(&ring).await, Vec::<i64>::new());
    assert!(!path.exists());

    record_range(&ring, 0..3).await;
    assert_eq!(line_count(&path), 3);
    assert_eq!(seconds(&ring).await, [0, 1, 2]);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.ends_with('\n'));
    assert!(text.lines().all(|line| line.starts_with(r#"{"at":"#)));
}

#[tokio::test]
async fn trims_to_max_entries_once_past_the_margin() {
    let dir = TempDir::new().unwrap();
    let (ring, path) = ring_in(&dir, 10);
    record_range(&ring, 0..11).await;
    assert_eq!(line_count(&path), 11, "within the one-line margin");
    assert_eq!(seconds(&ring).await, (1..11).collect::<Vec<_>>());

    ring.record(entry(11)).await.unwrap();
    assert_eq!(line_count(&path), 10);
    assert_eq!(seconds(&ring).await, (2..12).collect::<Vec<_>>());
    assert!(
        !dir.path()
            .join("state/stillwatch/history.jsonl.tmp")
            .exists()
    );

    for second in 12..60 {
        ring.record(entry(second)).await.unwrap();
        assert!(line_count(&path) <= 11);
    }
    assert_eq!(seconds(&ring).await, (50..60).collect::<Vec<_>>());
}

#[tokio::test]
async fn history_survives_a_restart() {
    let dir = TempDir::new().unwrap();
    let (ring, path) = ring_in(&dir, 10);
    record_range(&ring, 0..8).await;
    drop(ring);

    let (ring, _) = ring_in(&dir, 10);
    assert_eq!(seconds(&ring).await, (0..8).collect::<Vec<_>>());
    record_range(&ring, 8..12).await;
    assert_eq!(
        line_count(&path),
        10,
        "the line count picks up where it left off"
    );
    assert_eq!(seconds(&ring).await, (2..12).collect::<Vec<_>>());
}

#[tokio::test]
async fn a_truncated_last_line_is_skipped_and_not_glued_to() {
    let dir = TempDir::new().unwrap();
    let (ring, path) = ring_in(&dir, 10);
    record_range(&ring, 0..2).await;
    drop(ring);
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    file.write_all(br#"{"at":"2026-09-21T14:1"#).unwrap();
    drop(file);

    let (ring, _) = ring_in(&dir, 10);
    assert_eq!(seconds(&ring).await, [0, 1]);
    ring.record(entry(2)).await.unwrap();
    assert_eq!(seconds(&ring).await, [0, 1, 2]);
    assert_eq!(line_count(&path), 4);

    record_range(&ring, 3..10).await;
    assert_eq!(line_count(&path), 11);
    ring.record(entry(10)).await.unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        parse(text.as_bytes()).len(),
        10,
        "the trim drops the bad line"
    );
    assert_eq!(text.lines().count(), 10);
}

#[test]
fn unreadable_lines_are_skipped_with_a_warning() {
    let good = stillwatch_ipc::json::to_json(&entry(1)).unwrap();
    let mut bytes = format!("{good}\nnot json\n\n").into_bytes();
    bytes.extend_from_slice(&[0xff, 0xfe, b'\n']);
    bytes.extend_from_slice(good.as_bytes());

    let logs = Arc::new(Mutex::new(Vec::new()));
    let writer = Arc::clone(&logs);
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || Capture(Arc::clone(&writer)))
        .with_ansi(false)
        .finish();
    let parsed = tracing::subscriber::with_default(subscriber, || parse(&bytes));

    assert_eq!(parsed, [entry(1), entry(1)]);
    let logs = String::from_utf8(logs.lock().unwrap().clone()).unwrap();
    assert_eq!(logs.matches("skipping unreadable history line").count(), 2);
    assert!(logs.contains("line=2") && logs.contains("line=4"));
    assert!(
        !logs.contains("not json"),
        "line contents stay out of the log"
    );
}

struct Capture(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Capture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn disabled_history_writes_nothing_but_still_reads() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join(FILE_NAME);
    let ring = HistoryRing::new(&path, &config(false, 10));
    ring.record(entry(0)).await.unwrap();
    assert!(!path.exists());

    ring.apply_config(&config(true, 10)).await.unwrap();
    record_range(&ring, 1..3).await;
    ring.apply_config(&config(false, 10)).await.unwrap();
    ring.record(entry(3)).await.unwrap();
    assert_eq!(seconds(&ring).await, [1, 2]);
}

#[tokio::test]
async fn read_filters_by_since() {
    let dir = TempDir::new().unwrap();
    let (ring, _) = ring_in(&dir, 100);
    record_range(&ring, 0..5).await;
    let recent = ring.read(at(3)).await.unwrap();
    assert_eq!(recent, [entry(3), entry(4)]);
    assert_eq!(ring.read(at(60)).await.unwrap(), []);
}

#[tokio::test]
async fn a_smaller_max_entries_applies_on_reload() {
    let dir = TempDir::new().unwrap();
    let (ring, path) = ring_in(&dir, 100);
    record_range(&ring, 0..20).await;

    ring.apply_config(&config(true, 19)).await.unwrap();
    assert_eq!(line_count(&path), 20, "still within the margin");
    assert_eq!(seconds(&ring).await, (1..20).collect::<Vec<_>>());

    ring.apply_config(&config(true, 5)).await.unwrap();
    assert_eq!(line_count(&path), 5);
    assert_eq!(seconds(&ring).await, (15..20).collect::<Vec<_>>());
}

#[tokio::test]
async fn io_failures_are_backend_errors() {
    let dir = TempDir::new().unwrap();
    let ring = HistoryRing::new(dir.path(), &config(true, 10));
    let err = ring.record(entry(0)).await.unwrap_err();
    assert!(matches!(err, BackendError::Io(_)), "{err:?}");
    assert!(ring.read(at(0)).await.is_err());
}

#[tokio::test]
async fn usable_as_a_shared_sink() {
    let dir = TempDir::new().unwrap();
    let (ring, _) = ring_in(&dir, 10);
    let sink: Arc<dyn HistorySink> = Arc::new(ring.clone());
    let kinds = [HistoryKind::Prompt, HistoryKind::Blank];
    for kind in kinds {
        sink.record(HistoryEntry::new(at(0), kind)).await.unwrap();
    }
    let read: Vec<_> = ring.read(at(0)).await.unwrap();
    assert_eq!(read.iter().map(|e| e.kind).collect::<Vec<_>>(), kinds);
}
