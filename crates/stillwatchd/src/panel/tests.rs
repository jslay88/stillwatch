use std::fs;
use std::time::{Duration, Instant};

use stillwatch_core::panel::PanelRecord;
use stillwatch_ipc::json::from_json;

use super::*;

fn record(seconds: u64, uses: u32) -> PanelRecord {
    PanelRecord {
        screen_on_seconds: seconds,
        last_standby: None,
        overlay_uses: uses,
    }
}

#[test]
fn default_path_is_in_the_state_dir() {
    let expected = paths::state_dir().map(|dir| dir.join(FILE_NAME));
    assert_eq!(default_path(), expected);
}

#[test]
fn missing_file_loads_as_zero_and_is_not_created() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state").join(FILE_NAME);
    let mut store = PanelStore::new(&path);
    assert_eq!(store.load(), PanelRecord::default());
    assert!(!path.exists());
}

#[test]
fn a_change_is_written_after_the_debounce_and_reloads() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(FILE_NAME);
    let mut store = PanelStore::with_debounce(&path, Duration::from_secs(5));
    assert_eq!(store.load(), PanelRecord::default());

    let now = Instant::now();
    let saved = record(3 * 3600, 2);
    store.update(saved, now);
    store.flush_if_due(now).unwrap();
    assert!(!path.exists(), "inside the debounce");

    store.flush_if_due(now + Duration::from_secs(5)).unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert_eq!(from_json::<PanelRecord>(text.trim()).unwrap(), saved);

    let mut again = PanelStore::new(&path);
    assert_eq!(again.load(), saved);
    assert!(!path.with_extension("json.tmp").exists());
}

#[test]
fn flush_writes_without_waiting() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested").join(FILE_NAME);
    let mut store = PanelStore::new(&path);
    store.load();
    store.update(record(90, 1), Instant::now());
    store.flush().unwrap();
    let mut again = PanelStore::new(&path);
    assert_eq!(again.load(), record(90, 1));
}

#[test]
fn an_unchanged_record_is_not_rewritten() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(FILE_NAME);
    let mut store = PanelStore::new(&path);
    store.load();
    let now = Instant::now();
    store.update(PanelRecord::default(), now);
    store.flush().unwrap();
    assert!(!path.exists());
}

#[test]
fn a_corrupt_file_loads_as_zero() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(FILE_NAME);
    fs::write(&path, "{not json").unwrap();
    let mut store = PanelStore::new(&path);
    assert_eq!(store.load(), PanelRecord::default());

    store.update(record(10, 0), Instant::now());
    store.flush().unwrap();
    let mut again = PanelStore::new(&path);
    assert_eq!(again.load().screen_on_seconds, 10);
}
