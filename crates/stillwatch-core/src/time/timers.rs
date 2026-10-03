use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::command::Command;

/// Identifies a timer requested by the state machine.
///
/// At most one timer per id is pending: scheduling an id again replaces it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimerId {
    /// Next capture tick while monitoring or checking the ceiling.
    Capture,
    /// The prompt countdown ran out.
    PromptCountdown,
    /// The snooze expired.
    SnoozeExpiry,
    /// The session has been locked for `locked_blank_seconds`.
    LockedBlank,
    /// The re-blank grace period after a display woke without input.
    ReblankGrace,
    /// The dim phase of `dim_then_blank` is over.
    DimElapsed,
    /// Time to check whether the panel care reminder is due.
    PanelCareReminder,
    /// The last gamepad input is `idle.input_idle_minutes` old. Armed by the
    /// [`ActivityAggregator`](crate::activity::ActivityAggregator), not the
    /// state machine.
    GamepadIdle,
    /// Input arrived during a prompt and `answer_grace_seconds` passed
    /// without an answer.
    PromptAnswerGrace,
}

/// A queue of pending timers keyed by [`TimerId`].
///
/// Pure bookkeeping: the owner decides when to call
/// [`pop_due`](Self::pop_due), typically after sleeping until
/// [`next_deadline`](Self::next_deadline).
#[derive(Debug, Clone, Default)]
pub struct TimerQueue {
    pending: Vec<Pending>,
    next_seq: u64,
}

#[derive(Debug, Clone, Copy)]
struct Pending {
    id: TimerId,
    deadline: Instant,
    seq: u64,
}

impl TimerQueue {
    /// An empty queue.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Schedules `id` to fire at `deadline`, replacing any pending timer with
    /// the same id.
    pub fn schedule(&mut self, id: TimerId, deadline: Instant) {
        self.cancel(id);
        let seq = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1);
        self.pending.push(Pending { id, deadline, seq });
    }

    /// Schedules `id` to fire `after` from `now`. A delay too large to
    /// represent never fires, so the timer is only cancelled.
    pub fn schedule_after(&mut self, id: TimerId, now: Instant, after: Duration) {
        match now.checked_add(after) {
            Some(deadline) => self.schedule(id, deadline),
            None => {
                self.cancel(id);
            }
        }
    }

    /// Cancels `id`. Returns whether it was pending.
    pub fn cancel(&mut self, id: TimerId) -> bool {
        let before = self.pending.len();
        self.pending.retain(|timer| timer.id != id);
        self.pending.len() != before
    }

    /// Applies a `SetTimer` or `CancelTimer` command issued at `now`.
    /// Returns whether `command` was one of those two.
    pub fn apply(&mut self, now: Instant, command: &Command) -> bool {
        match command {
            Command::SetTimer { id, after } => self.schedule_after(*id, now, *after),
            Command::CancelTimer(id) => {
                self.cancel(*id);
            }
            _ => return false,
        }
        true
    }

    /// Whether `id` is pending.
    #[must_use]
    pub fn contains(&self, id: TimerId) -> bool {
        self.pending.iter().any(|timer| timer.id == id)
    }

    /// When `id` fires, if pending.
    #[must_use]
    pub fn deadline(&self, id: TimerId) -> Option<Instant> {
        self.pending
            .iter()
            .find(|timer| timer.id == id)
            .map(|timer| timer.deadline)
    }

    /// The earliest pending deadline.
    #[must_use]
    pub fn next_deadline(&self) -> Option<Instant> {
        self.pending.iter().map(|timer| timer.deadline).min()
    }

    /// Removes and returns every timer due at `now`, earliest deadline first.
    /// Timers with equal deadlines fire in the order they were scheduled.
    pub fn pop_due(&mut self, now: Instant) -> Vec<TimerId> {
        let (mut due, pending): (Vec<_>, Vec<_>) =
            self.pending.iter().partition(|timer| timer.deadline <= now);
        self.pending = pending;
        due.sort_by_key(|timer| (timer.deadline, timer.seq));
        due.into_iter().map(|timer| timer.id).collect()
    }

    /// Number of pending timers.
    #[must_use]
    pub fn len(&self) -> usize {
        self.pending.len()
    }

    /// Whether no timers are pending.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::{Clock, FakeClock};

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    #[test]
    fn advancing_a_fake_clock_fires_timers_in_deadline_order() {
        let clock = FakeClock::new();
        let mut timers = TimerQueue::new();
        timers.schedule_after(TimerId::SnoozeExpiry, clock.now(), secs(30));
        timers.schedule_after(TimerId::Capture, clock.now(), secs(10));
        timers.schedule_after(TimerId::PromptCountdown, clock.now(), secs(20));
        assert_eq!(timers.len(), 3);
        assert_eq!(timers.next_deadline(), clock.now().checked_add(secs(10)));

        clock.advance(secs(5));
        assert_eq!(timers.pop_due(clock.now()), []);

        clock.advance(secs(20));
        assert_eq!(
            timers.pop_due(clock.now()),
            vec![TimerId::Capture, TimerId::PromptCountdown]
        );

        clock.advance(secs(5));
        assert_eq!(timers.pop_due(clock.now()), vec![TimerId::SnoozeExpiry]);
        assert!(timers.is_empty());
        assert_eq!(timers.next_deadline(), None);
    }

    #[test]
    fn equal_deadlines_fire_in_scheduling_order() {
        let now = FakeClock::new().now();
        let mut timers = TimerQueue::new();
        timers.schedule_after(TimerId::DimElapsed, now, secs(1));
        timers.schedule_after(TimerId::LockedBlank, now, secs(1));
        timers.schedule_after(TimerId::ReblankGrace, now, secs(1));
        assert_eq!(
            timers.pop_due(now + secs(1)),
            vec![
                TimerId::DimElapsed,
                TimerId::LockedBlank,
                TimerId::ReblankGrace
            ]
        );
    }

    #[test]
    fn rescheduling_replaces_and_cancel_removes() {
        let now = FakeClock::new().now();
        let mut timers = TimerQueue::new();
        timers.schedule_after(TimerId::Capture, now, secs(5));
        timers.schedule_after(TimerId::Capture, now, secs(60));
        assert_eq!(timers.len(), 1);
        assert_eq!(timers.pop_due(now + secs(5)), []);
        assert!(timers.contains(TimerId::Capture));

        assert!(timers.cancel(TimerId::Capture));
        assert!(!timers.cancel(TimerId::Capture));
        assert!(!timers.contains(TimerId::Capture));
    }

    #[test]
    fn applies_timer_commands_and_ignores_others() {
        let now = FakeClock::new().now();
        let mut timers = TimerQueue::new();
        let set = Command::SetTimer {
            id: TimerId::Capture,
            after: secs(60),
        };
        assert!(timers.apply(now, &set));
        assert_eq!(timers.deadline(TimerId::Capture), Some(now + secs(60)));
        assert_eq!(timers.deadline(TimerId::SnoozeExpiry), None);
        assert!(!timers.apply(now, &Command::DismissPrompt));
        assert!(timers.apply(now, &Command::CancelTimer(TimerId::Capture)));
        assert!(timers.is_empty());
    }

    #[test]
    fn unrepresentable_delay_never_fires() {
        let now = FakeClock::new().now();
        let mut timers = TimerQueue::new();
        timers.schedule_after(TimerId::PanelCareReminder, now, secs(1));
        timers.schedule_after(TimerId::PanelCareReminder, now, Duration::MAX);
        assert!(timers.is_empty());
    }

    #[test]
    fn timer_ids_serialize_as_snake_case() {
        let json = serde_json::to_string(&TimerId::PromptCountdown).unwrap();
        assert_eq!(json, r#""prompt_countdown""#);
    }
}
