use std::time::Duration;

use jiff::{SignedDuration, Timestamp};

use super::*;
use crate::config::PanelCareConfig;
use crate::event::PowerKind;
use crate::time::FakeClock;

fn config(edit: impl FnOnce(&mut PanelCareConfig)) -> PanelCareConfig {
    let mut config = PanelCareConfig::default();
    edit(&mut config);
    config
}

fn tracker(edit: impl FnOnce(&mut PanelCareConfig)) -> (PanelTracker, FakeClock) {
    let clock = FakeClock::with_wall(Timestamp::from_second(1_700_000_000).unwrap());
    (PanelTracker::new(config(edit)), clock)
}

fn on(tracker: &mut PanelTracker, clock: &FakeClock, output: &str) {
    tracker.power(clock, output, PowerKind::Dpms, true);
}

fn standby(tracker: &mut PanelTracker, clock: &FakeClock, output: &str, kind: PowerKind) {
    tracker.power(clock, output, kind, false);
}

#[test]
fn screen_on_time_accumulates_while_the_panel_is_on() {
    let (mut tracker, clock) = tracker(|_| {});
    on(&mut tracker, &clock, "HDMI-A-1");
    clock.advance(Duration::from_hours(2));
    assert_eq!(tracker.screen_on(&clock), Duration::from_hours(2));
    clock.advance(Duration::from_mins(30));
    assert_eq!(tracker.screen_on(&clock), Duration::from_mins(150));
    assert_eq!(tracker.record(&clock).screen_on_seconds, 150 * 60);
}

#[test]
fn a_short_standby_does_not_reset() {
    let (mut tracker, clock) = tracker(|c| c.min_standby_minutes = 10);
    on(&mut tracker, &clock, "HDMI-A-1");
    clock.advance(Duration::from_hours(1));
    standby(&mut tracker, &clock, "HDMI-A-1", PowerKind::Dpms);
    clock.advance(Duration::from_mins(9));
    tracker.tick(&clock);
    assert_eq!(tracker.screen_on(&clock), Duration::from_hours(1));
    assert_eq!(tracker.last_standby(), None);

    on(&mut tracker, &clock, "HDMI-A-1");
    clock.advance(Duration::from_hours(1));
    assert_eq!(tracker.screen_on(&clock), Duration::from_hours(2));
}

#[test]
fn a_long_standby_resets_and_records_when_it_qualified() {
    let (mut tracker, clock) = tracker(|c| c.min_standby_minutes = 10);
    on(&mut tracker, &clock, "HDMI-A-1");
    clock.advance(Duration::from_hours(3));
    standby(&mut tracker, &clock, "HDMI-A-1", PowerKind::Ddc);
    let update = tracker.tick(&clock);
    assert!(update.reminder.is_none());
    assert_eq!(update.check_after, Some(Duration::from_mins(10)));

    clock.advance(Duration::from_mins(10));
    let qualified = clock.wall_now();
    tracker.tick(&clock);
    assert_eq!(tracker.screen_on(&clock), Duration::ZERO);
    assert_eq!(tracker.last_standby(), Some(qualified));

    on(&mut tracker, &clock, "HDMI-A-1");
    clock.advance(Duration::from_mins(5));
    assert_eq!(tracker.screen_on(&clock), Duration::from_mins(5));
    assert_eq!(tracker.last_standby(), Some(qualified));
}

#[test]
fn a_late_tick_stamps_the_standby_when_it_actually_qualified() {
    let (mut tracker, clock) = tracker(|c| c.min_standby_minutes = 10);
    on(&mut tracker, &clock, "HDMI-A-1");
    standby(&mut tracker, &clock, "HDMI-A-1", PowerKind::Dpms);
    clock.advance(Duration::from_mins(12));
    let noticed = clock.wall_now();
    tracker.tick(&clock);
    let expected = noticed
        .checked_sub(SignedDuration::from_mins(2))
        .expect("stamp");
    assert_eq!(tracker.last_standby(), Some(expected));
}

#[test]
fn the_overlay_counts_as_on_and_does_not_reset() {
    let (mut tracker, clock) = tracker(|c| c.min_standby_minutes = 10);
    on(&mut tracker, &clock, "HDMI-A-1");
    clock.advance(Duration::from_hours(1));
    standby(&mut tracker, &clock, "HDMI-A-1", PowerKind::Dpms);
    clock.advance(Duration::from_mins(5));
    tracker.power(&clock, "HDMI-A-1", PowerKind::Overlay, false);
    assert_eq!(tracker.overlay_uses(), 1);
    clock.advance(Duration::from_hours(1));
    assert_eq!(tracker.screen_on(&clock), Duration::from_hours(2));
    assert_eq!(tracker.last_standby(), None);

    tracker.power(&clock, "HDMI-A-1", PowerKind::Overlay, false);
    assert_eq!(tracker.overlay_uses(), 1);
    tracker.power(&clock, "DP-1", PowerKind::Overlay, false);
    assert_eq!(tracker.overlay_uses(), 2);
}

#[test]
fn one_output_in_standby_does_not_reset_while_another_is_on() {
    let (mut tracker, clock) = tracker(|c| c.min_standby_minutes = 10);
    on(&mut tracker, &clock, "HDMI-A-1");
    on(&mut tracker, &clock, "DP-1");
    clock.advance(Duration::from_mins(20));
    standby(&mut tracker, &clock, "HDMI-A-1", PowerKind::Dpms);
    clock.advance(Duration::from_mins(30));
    tracker.tick(&clock);
    assert_eq!(tracker.screen_on(&clock), Duration::from_mins(50));
    assert_eq!(tracker.last_standby(), None);
}

#[test]
fn the_reminder_fires_once_past_the_threshold_and_again_after_the_snooze() {
    let (mut tracker, clock) = tracker(|c| c.reminder_hours = 4);
    on(&mut tracker, &clock, "HDMI-A-1");
    clock.advance(Duration::from_hours(4));
    assert!(tracker.tick(&clock).reminder.is_none());
    assert!(!tracker.trigger_due(&clock));

    clock.advance(Duration::from_secs(1));
    let update = tracker.tick(&clock);
    assert_eq!(
        update.reminder,
        Some(Duration::from_hours(4) + Duration::from_secs(1))
    );
    assert!(tracker.trigger_due(&clock));
    assert!(tracker.tick(&clock).reminder.is_none());

    clock.advance(Duration::from_hours(4));
    let again = tracker.tick(&clock);
    assert!(again.reminder.is_some());
}

#[test]
fn a_reset_lets_the_reminder_fire_again_before_the_snooze_would_have() {
    let (mut tracker, clock) = tracker(|c| {
        c.reminder_hours = 4;
        c.min_standby_minutes = 10;
    });
    on(&mut tracker, &clock, "HDMI-A-1");
    clock.advance(Duration::from_hours(4) + Duration::from_secs(1));
    assert!(tracker.tick(&clock).reminder.is_some());

    standby(&mut tracker, &clock, "HDMI-A-1", PowerKind::Dpms);
    clock.advance(Duration::from_mins(10));
    tracker.tick(&clock);
    assert_eq!(tracker.screen_on(&clock), Duration::ZERO);

    on(&mut tracker, &clock, "HDMI-A-1");
    clock.advance(Duration::from_hours(4) + Duration::from_secs(1));
    assert!(tracker.tick(&clock).reminder.is_some());
}

#[test]
fn disabled_tracking_ignores_power_reminders_and_the_trigger() {
    let (mut tracker, clock) = tracker(|c| c.enabled = false);
    on(&mut tracker, &clock, "HDMI-A-1");
    clock.advance(Duration::from_hours(5));
    let update = tracker.tick(&clock);
    assert_eq!(update, PanelUpdate::idle());
    assert_eq!(tracker.screen_on(&clock), Duration::ZERO);
    assert!(!tracker.trigger_due(&clock));
    assert_eq!(tracker.overlay_uses(), 0);
}

#[test]
fn disabling_freezes_the_total() {
    let (mut tracker, clock) = tracker(|_| {});
    on(&mut tracker, &clock, "HDMI-A-1");
    clock.advance(Duration::from_hours(1));
    let frozen = tracker.screen_on(&clock);
    tracker.set_config(config(|c| c.enabled = false), &clock);
    clock.advance(Duration::from_hours(2));
    tracker.power(&clock, "HDMI-A-1", PowerKind::Overlay, false);
    assert_eq!(tracker.screen_on(&clock), frozen);
    assert_eq!(tracker.overlay_uses(), 0);
    assert!(!tracker.trigger_due(&clock));
}

#[test]
fn reminder_disabled_still_marks_the_trigger_due() {
    let (mut tracker, clock) = tracker(|c| {
        c.reminder_enabled = false;
        c.reminder_hours = 1;
    });
    on(&mut tracker, &clock, "HDMI-A-1");
    clock.advance(Duration::from_hours(1) + Duration::from_secs(1));
    assert!(tracker.tick(&clock).reminder.is_none());
    assert!(tracker.trigger_due(&clock));
}

#[test]
fn restore_keeps_the_counters_and_keeps_accumulating() {
    let (mut tracker, clock) = tracker(|_| {});
    on(&mut tracker, &clock, "HDMI-A-1");
    clock.advance(Duration::from_secs(90));
    let saved = tracker.record(&clock);
    let mut restored = PanelTracker::restore(config(|_| {}), saved);
    assert_eq!(restored.record(&clock), saved);
    on(&mut restored, &clock, "HDMI-A-1");
    clock.advance(Duration::from_secs(30));
    assert_eq!(restored.screen_on(&clock), Duration::from_secs(120));
}
