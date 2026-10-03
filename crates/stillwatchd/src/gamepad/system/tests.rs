use futures_util::stream;

use super::*;

fn at(kind: EventType, path: &str, joystick: bool) -> Option<Hotplug> {
    hotplug(kind, Some(Path::new(path)), joystick)
}

fn qualifies(path: &str) -> bool {
    event_node(Some(Path::new(path))).is_some()
}

#[test]
fn only_event_nodes_qualify() {
    assert!(qualifies("/dev/input/event7"));
    assert!(!qualifies("/dev/input/js0"));
    assert!(!qualifies("/dev/input/mouse1"));
    assert_eq!(event_node(None), None);
}

#[test]
fn joystick_adds_and_changes_open_the_node() {
    let added = Some(Hotplug::Added("/dev/input/event7".into()));
    assert_eq!(at(EventType::Add, "/dev/input/event7", true), added);
    assert_eq!(at(EventType::Change, "/dev/input/event7", true), added);
}

#[test]
fn non_joysticks_and_other_nodes_are_skipped() {
    assert_eq!(at(EventType::Add, "/dev/input/event3", false), None);
    assert_eq!(at(EventType::Add, "/dev/input/js0", true), None);
    assert_eq!(hotplug(EventType::Add, None, true), None);
    assert_eq!(at(EventType::Bind, "/dev/input/event7", true), None);
    assert_eq!(at(EventType::Unknown, "/dev/input/event7", true), None);
}

#[test]
fn removals_skip_the_joystick_check() {
    let removed = Some(Hotplug::Removed("/dev/input/event7".into()));
    assert_eq!(at(EventType::Remove, "/dev/input/event7", false), removed);
    assert_eq!(at(EventType::Remove, "/dev/input/js0", false), None);
}

#[tokio::test]
async fn forwards_changes_then_the_error_that_ended_them() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let changes = stream::iter(vec![
        Ok(None),
        Ok(Some(Hotplug::Added("/dev/input/event7".into()))),
        Err(io::Error::other("netlink overrun")),
        Ok(Some(Hotplug::Removed("/dev/input/event7".into()))),
    ]);
    forward_changes(changes, &tx).await;
    assert_eq!(
        rx.recv().await,
        Some(Ok(Hotplug::Added("/dev/input/event7".into())))
    );
    assert_eq!(
        rx.recv().await,
        Some(Err(BackendError::Io("netlink overrun".into())))
    );
    drop(tx);
    assert_eq!(rx.recv().await, None);
}

#[tokio::test]
async fn a_closed_socket_is_a_disconnect() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    forward_changes(stream::empty(), &tx).await;
    assert!(matches!(
        rx.recv().await,
        Some(Err(BackendError::Disconnected(_)))
    ));
}

#[tokio::test]
async fn stops_when_nobody_listens() {
    let (tx, rx) = mpsc::unbounded_channel();
    drop(rx);
    forward_changes(stream::pending(), &tx).await;
    assert!(tx.is_closed());
}

#[test]
fn enumeration_only_returns_sorted_event_nodes() {
    let Ok(nodes) = UdevPlatform.joysticks() else {
        return;
    };
    assert!(nodes.iter().all(|node| event_node(Some(node)).is_some()));
    assert!(nodes.is_sorted());
}

#[tokio::test]
async fn the_monitor_starts_or_reports_why_not() {
    match UdevPlatform.monitor().await {
        Ok(changes) => drop(changes),
        Err(err) => eprintln!("udev monitor unavailable here: {err}"),
    }
}

#[tokio::test]
async fn opening_through_the_platform_reports_missing_nodes() {
    let err = UdevPlatform
        .open(Path::new("/dev/input/event-stillwatch-missing"))
        .err()
        .unwrap();
    assert_eq!(err.kind(), io::ErrorKind::NotFound);
}
