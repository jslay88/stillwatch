use stillwatch_core::event::SessionEvent::{Locked, PrepareForSleep, ResumedFromSleep, Unlocked};

use super::*;

const SOURCES: [Source; 3] = [Source::LockedHint, Source::ScreenSaver, Source::Request];

fn unlocked() -> Snapshot {
    Snapshot {
        locked_hint: Some(false),
        screensaver: Some(false),
        sleeping: false,
    }
}

fn started(snapshot: Snapshot) -> Tracker {
    let mut tracker = Tracker::default();
    assert_eq!(tracker.start(snapshot), []);
    tracker
}

fn feed(tracker: &mut Tracker, reports: &[Source], locked: bool) -> Vec<SessionEvent> {
    reports
        .iter()
        .filter_map(|source| tracker.lock(*source, locked))
        .collect()
}

/// Every sequence of up to `max_len` reports, repeats included.
fn sequences(max_len: usize) -> Vec<Vec<Source>> {
    let mut all = vec![Vec::new()];
    let mut frontier = vec![Vec::new()];
    for _ in 0..max_len {
        frontier = frontier
            .iter()
            .flat_map(|seq: &Vec<Source>| {
                SOURCES.iter().map(move |source| {
                    let mut next = seq.clone();
                    next.push(*source);
                    next
                })
            })
            .collect();
        all.extend(frontier.iter().cloned());
    }
    all
}

fn levels(reports: &[Source]) -> Vec<Source> {
    let mut levels: Vec<Source> = reports
        .iter()
        .copied()
        .filter(|source| *source != Source::Request)
        .collect();
    levels.sort_by_key(|source| *source as u8);
    levels.dedup();
    levels
}

#[test]
fn every_ordering_and_repeat_of_a_lock_and_unlock_gives_one_event_each() {
    let all = sequences(5);
    for lock in all.iter().filter(|seq| !seq.is_empty()) {
        let reported = levels(lock);
        // A level source only says "unlocked" again after it said "locked":
        // logind and the screensaver only signal changes (plus duplicates).
        let unlocks = all.iter().filter(|seq| {
            !seq.is_empty() && levels(seq).iter().all(|source| reported.contains(source))
        });
        for unlock in unlocks {
            let mut tracker = started(unlocked());
            assert_eq!(feed(&mut tracker, lock, true), [Locked], "{lock:?}");
            assert_eq!(
                feed(&mut tracker, unlock, false),
                [Unlocked],
                "{lock:?} then {unlock:?}"
            );
        }
    }
}

#[test]
fn a_missing_screensaver_doesnt_hold_anything_back() {
    let mut tracker = started(Snapshot {
        screensaver: None,
        ..unlocked()
    });
    assert_eq!(tracker.lock(Source::LockedHint, true), Some(Locked));
    assert_eq!(tracker.lock(Source::LockedHint, false), Some(Unlocked));
}

#[test]
fn a_stale_repeat_after_another_source_moved_on_is_dropped() {
    let mut tracker = started(unlocked());
    assert_eq!(tracker.lock(Source::LockedHint, true), Some(Locked));
    assert_eq!(tracker.lock(Source::ScreenSaver, true), None);
    assert_eq!(tracker.lock(Source::ScreenSaver, false), Some(Unlocked));
    assert_eq!(tracker.lock(Source::LockedHint, true), None);
    assert_eq!(tracker.lock(Source::LockedHint, false), None);
}

#[test]
fn repeated_lock_requests_count_once_the_state_moved() {
    let mut tracker = started(unlocked());
    assert_eq!(tracker.lock(Source::Request, true), Some(Locked));
    assert_eq!(tracker.lock(Source::Request, true), None);
    assert_eq!(tracker.lock(Source::ScreenSaver, false), None);
    assert_eq!(tracker.lock(Source::Request, false), Some(Unlocked));
    assert_eq!(tracker.lock(Source::Request, true), Some(Locked));
}

#[test]
fn the_first_start_is_silent_and_sets_the_state() {
    let mut tracker = started(Snapshot {
        locked_hint: Some(true),
        ..unlocked()
    });
    assert_eq!(tracker.lock(Source::ScreenSaver, true), None);
    assert_eq!(tracker.lock(Source::LockedHint, false), Some(Unlocked));
}

#[test]
fn either_source_locked_means_locked() {
    let cases = [
        (None, None, None),
        (Some(false), None, Some(false)),
        (None, Some(true), Some(true)),
        (Some(true), Some(false), Some(true)),
        (Some(false), Some(false), Some(false)),
    ];
    for (locked_hint, screensaver, expected) in cases {
        let snapshot = Snapshot {
            locked_hint,
            screensaver,
            sleeping: false,
        };
        assert_eq!(snapshot.locked(), expected, "{snapshot:?}");
    }
}

#[test]
fn an_unreadable_start_reports_the_first_signal() {
    let mut tracker = started(Snapshot::default());
    assert_eq!(tracker.lock(Source::ScreenSaver, false), Some(Unlocked));
    assert_eq!(tracker.lock(Source::LockedHint, true), Some(Locked));
}

#[test]
fn a_restart_reports_what_changed_while_disconnected() {
    let mut tracker = started(unlocked());
    tracker.stop();
    let locked = Snapshot {
        locked_hint: Some(true),
        ..unlocked()
    };
    assert_eq!(tracker.start(locked), [Locked]);
    tracker.stop();
    assert_eq!(tracker.start(locked), []);
    tracker.stop();
    assert_eq!(tracker.start(unlocked()), [Unlocked]);
}

#[test]
fn a_restart_reseeds_each_source() {
    let mut tracker = started(unlocked());
    assert_eq!(tracker.lock(Source::ScreenSaver, true), Some(Locked));
    tracker.stop();
    assert_eq!(
        tracker.start(Snapshot {
            screensaver: None,
            ..unlocked()
        }),
        [Unlocked]
    );
    assert_eq!(tracker.lock(Source::ScreenSaver, true), Some(Locked));
}

#[test]
fn is_locked_sets_the_baseline_only_between_watches() {
    let mut tracker = Tracker::default();
    tracker.told(true);
    assert_eq!(tracker.start(unlocked()), [Unlocked]);
    tracker.told(true);
    assert_eq!(tracker.lock(Source::LockedHint, true), Some(Locked));
    tracker.stop();
    tracker.told(false);
    assert_eq!(tracker.start(unlocked()), []);
}

#[test]
fn sleep_is_reported_once_per_edge() {
    let mut tracker = started(unlocked());
    assert_eq!(tracker.sleep(false), None);
    assert_eq!(tracker.sleep(true), Some(PrepareForSleep));
    assert_eq!(tracker.sleep(true), None);
    assert_eq!(tracker.sleep(false), Some(ResumedFromSleep));
    assert_eq!(tracker.sleep(false), None);
}

#[test]
fn a_resume_missed_while_disconnected_is_reported_on_restart() {
    let mut tracker = started(unlocked());
    assert_eq!(tracker.sleep(true), Some(PrepareForSleep));
    tracker.stop();
    assert_eq!(tracker.start(unlocked()), [ResumedFromSleep]);
    tracker.stop();
    let sleeping = Snapshot {
        sleeping: true,
        locked_hint: Some(true),
        ..unlocked()
    };
    assert_eq!(tracker.start(sleeping), [Locked, PrepareForSleep]);
}
