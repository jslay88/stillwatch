use std::fs::Permissions;
use std::os::unix::fs::{PermissionsExt as _, symlink};

use notify::event::{AccessKind, AccessMode, CreateKind, ModifyKind};

use super::*;

/// A directory nobody but root can read. `None` when running as root,
/// where permissions don't stop anything.
fn locked_dir(root: &Path) -> Option<PathBuf> {
    let dir = root.join("locked");
    std::fs::create_dir(&dir).unwrap();
    std::fs::set_permissions(&dir, Permissions::from_mode(0o000)).unwrap();
    if std::fs::read_dir(&dir).is_ok() {
        return None;
    }
    Some(dir)
}

fn unlock(dir: &Path) {
    std::fs::set_permissions(dir, Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn reads_dont_count_as_changes() {
    assert!(is_read(EventKind::Access(AccessKind::Open(
        AccessMode::Any
    ))));
    assert!(is_read(EventKind::Access(AccessKind::Close(
        AccessMode::Read
    ))));
    assert!(is_read(EventKind::Access(AccessKind::Read)));
    assert!(!is_read(EventKind::Access(AccessKind::Close(
        AccessMode::Write
    ))));
    assert!(!is_read(EventKind::Modify(ModifyKind::Any)));
    assert!(!is_read(EventKind::Create(CreateKind::File)));
}

#[tokio::test]
async fn spawn_triggers_after_the_default_debounce() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("config.toml");
    let mut watcher = ConfigWatcher::spawn(&path).unwrap();
    let started = Instant::now();
    std::fs::write(&path, "").unwrap();
    let trigger = tokio::time::timeout(Duration::from_secs(10), watcher.next())
        .await
        .unwrap();
    assert_eq!(trigger, Some(ReloadTrigger::FileChanged));
    assert!(started.elapsed() >= DEFAULT_DEBOUNCE);
}

#[tokio::test]
async fn an_unwatchable_directory_fails_to_start() {
    let tmp = tempfile::tempdir().unwrap();
    let Some(locked) = locked_dir(tmp.path()) else {
        return;
    };
    let error = ConfigWatcher::spawn(&locked.join("config.toml")).unwrap_err();
    unlock(&locked);
    let WatchError::Watch { path, .. } = &error else {
        panic!("expected a watch error, got {error:?}");
    };
    assert_eq!(path, &std::fs::canonicalize(&locked).unwrap());
    assert!(error.to_string().starts_with("can't watch "), "{error}");
}

#[tokio::test]
async fn retargeting_to_an_unwatchable_directory_keeps_the_rest() {
    let tmp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(tmp.path()).unwrap();
    std::fs::create_dir(root.join("stillwatch")).unwrap();
    let path = root.join("stillwatch/config.toml");
    let mut watches = Watches::new(&path, notify::recommended_watcher(|_| {}).unwrap());
    watches.retarget();
    assert_eq!(watches.watched, BTreeSet::from([root.join("stillwatch")]));

    let Some(locked) = locked_dir(&root) else {
        return;
    };
    symlink(locked.join("config.toml"), &path).unwrap();
    watches.retarget();
    unlock(&locked);
    assert_eq!(watches.watched, BTreeSet::from([root.join("stillwatch")]));
}

#[tokio::test]
async fn rescans_and_watcher_errors_count_as_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("config.toml");
    let mut watches = Watches::new(&path, notify::recommended_watcher(|_| {}).unwrap());
    let rescan = Event::new(EventKind::Other).set_flag(notify::event::Flag::Rescan);
    assert!(watches.handle(&rescan));
    let unrelated = Event::new(EventKind::Create(CreateKind::File)).add_path(tmp.path().join("x"));
    assert!(!watches.handle(&unrelated));

    let (events_tx, events) = mpsc::unbounded_channel();
    let (triggers_tx, mut triggers) = mpsc::channel(1);
    let task = tokio::spawn(run(watches, events, triggers_tx, Duration::from_millis(10)));
    events_tx
        .send(Err(notify::Error::generic("queue overflow")))
        .unwrap();
    assert_eq!(triggers.recv().await, Some(ReloadTrigger::FileChanged));
    drop(events_tx);
    task.await.unwrap();
}
