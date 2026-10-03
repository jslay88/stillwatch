//! Merges logind's and the screensaver's lock signals into exactly one
//! `Locked` / `Unlocked` per real transition, and suspend notices into one
//! `PrepareForSleep` / `ResumedFromSleep` each.
//!
//! One lock usually arrives several times: logind's `Lock` request, the
//! screensaver's `ActiveChanged`, and `LockedHint`, in any order, on two
//! buses, sometimes more than once. The first report of a new state wins and
//! the rest are dropped.

use stillwatch_core::event::SessionEvent;

/// Where a lock report came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Source {
    /// The logind session's `LockedHint` property.
    LockedHint,
    /// `org.freedesktop.ScreenSaver.ActiveChanged`.
    ScreenSaver,
    /// The logind session's `Lock` / `Unlock` signals. These are requests to
    /// the lock screen rather than states, so a repeat is a new request.
    Request,
}

/// The lock and sleep state read when a watch starts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Snapshot {
    /// `LockedHint`, if the session could be read.
    pub locked_hint: Option<bool>,
    /// `ScreenSaver.GetActive`, if a screensaver answered.
    pub screensaver: Option<bool>,
    /// logind's `PreparingForSleep`.
    pub sleeping: bool,
}

impl Snapshot {
    /// Locked if either source says so; `None` when neither could be read.
    pub fn locked(self) -> Option<bool> {
        match (self.locked_hint, self.screensaver) {
            (None, None) => None,
            (hint, active) => Some(hint == Some(true) || active == Some(true)),
        }
    }
}

/// What the consumer has been told, and the last value of each lock source.
///
/// It outlives a single watch, so a watch that starts after a reconnect can
/// report what changed while the bus was gone.
#[derive(Debug, Default)]
pub(crate) struct Tracker {
    /// The lock state the consumer last learned, from `is_locked` or an
    /// event.
    locked: Option<bool>,
    sleeping: bool,
    locked_hint: Option<bool>,
    screensaver: Option<bool>,
    watching: bool,
}

impl Tracker {
    /// A watch started and read `snapshot`. Returns what changed since the
    /// consumer last heard, which is nothing on the first start unless
    /// `is_locked` was called before it.
    pub fn start(&mut self, snapshot: Snapshot) -> Vec<SessionEvent> {
        self.watching = true;
        self.locked_hint = snapshot.locked_hint;
        self.screensaver = snapshot.screensaver;
        let mut events = Vec::new();
        if let Some(locked) = snapshot.locked() {
            if self.locked.is_some_and(|known| known != locked) {
                events.push(lock_event(locked));
            }
            self.locked = Some(locked);
        }
        events.extend(self.sleep(snapshot.sleeping));
        events
    }

    /// The watch ended.
    pub fn stop(&mut self) {
        self.watching = false;
    }

    /// `is_locked` returned `locked`. While a watch runs its events are the
    /// record, so this only counts between watches.
    pub fn told(&mut self, locked: bool) {
        if !self.watching {
            self.locked = Some(locked);
        }
    }

    /// `source` says the session is (un)locked.
    pub fn lock(&mut self, source: Source, locked: bool) -> Option<SessionEvent> {
        let level = match source {
            Source::LockedHint => Some(&mut self.locked_hint),
            Source::ScreenSaver => Some(&mut self.screensaver),
            Source::Request => None,
        };
        if let Some(level) = level {
            if *level == Some(locked) {
                return None;
            }
            *level = Some(locked);
        }
        if self.locked == Some(locked) {
            return None;
        }
        self.locked = Some(locked);
        Some(lock_event(locked))
    }

    /// logind's `PrepareForSleep(sleeping)`.
    pub fn sleep(&mut self, sleeping: bool) -> Option<SessionEvent> {
        if self.sleeping == sleeping {
            return None;
        }
        self.sleeping = sleeping;
        Some(if sleeping {
            SessionEvent::PrepareForSleep
        } else {
            SessionEvent::ResumedFromSleep
        })
    }
}

const fn lock_event(locked: bool) -> SessionEvent {
    if locked {
        SessionEvent::Locked
    } else {
        SessionEvent::Unlocked
    }
}

#[cfg(test)]
mod tests;
