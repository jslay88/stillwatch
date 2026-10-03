//! Status, config reloads, media, and events that arrive in the wrong state.

use std::time::Duration;

use super::transitions::reach;
use super::{config, records};
use crate::command::Command;
use crate::config::Config;
use crate::event::{ControlCommand, Event, SessionEvent};
use crate::history::HistoryKind;
use crate::luma::OutputInfo;
use crate::mocks::{Harness, Observation, ScriptedDetector};
use crate::prompt::PromptOutcome;
use crate::state::{State, StateMachine, StatusSnapshot};
use crate::time::{Clock, FakeClock, TimerId};

#[test]
fn a_new_machine_is_active_with_nothing_to_do() {
    let clock = FakeClock::new();
    let detector = Box::new(ScriptedDetector::new());
    let (machine, commands) = StateMachine::new(&Config::default(), detector, clock.now());
    assert_eq!(commands, vec![]);
    assert_eq!(machine.state(), State::Active);
    assert_eq!(machine.config(), &Config::default());
    assert_eq!(
        machine.status(clock.now()),
        StatusSnapshot {
            state: State::Active,
            in_state: Duration::ZERO,
            snooze_remaining: None,
            idle: false,
            locked: false,
            media_playing: false,
            last_detection: None,
        }
    );
}

#[test]
fn status_tracks_time_in_state_and_snooze_remaining() {
    let mut h = Harness::new();
    h.advance(Duration::from_secs(30));
    assert_eq!(h.status().in_state, Duration::from_secs(30));
    h.to_prompting();
    h.answer(PromptOutcome::Snooze(Duration::from_mins(15)));
    h.advance(Duration::from_mins(5));
    let status = h.status();
    assert_eq!(status.state, State::Snoozed);
    assert_eq!(status.in_state, Duration::from_mins(5));
    assert_eq!(status.snooze_remaining, Some(Duration::from_mins(10)));
    assert!(status.idle);
    assert!(status.last_detection.is_some_and(|d| d.stale));
}

#[test]
fn status_reports_lock_and_media() {
    let mut h = Harness::new();
    h.send(Event::Media {
        playing: vec!["spotify".into()],
    });
    assert!(!h.status().media_playing);
    h.send(Event::Media {
        playing: vec!["spotify".into(), "firefox.instance_1_42".into()],
    });
    h.send(SessionEvent::Locked);
    let status = h.status();
    assert!(status.media_playing && status.locked);
    let entry = super::transition_record(&h.send(SessionEvent::Unlocked));
    assert!(entry.context.media_playing && !entry.context.locked);
}

#[test]
fn playing_players_reach_the_detector() {
    let mut h = Harness::new();
    h.send(Event::Media {
        playing: vec!["mpv".into()],
    });
    h.idle();
    h.capture(false);
    assert_eq!(
        h.detector().observations(),
        vec![Observation {
            outputs: vec!["HDMI-A-1".into()],
            playing: vec!["mpv".into()],
        }]
    );
}

#[test]
fn reload_rearms_the_capture_timer_and_records_itself() {
    let mut h = reach(State::Monitoring);
    h.advance(Duration::from_secs(20));
    let mut config = Config::default();
    config.stale.check_interval_seconds = 10;
    let commands = h.apply_config(&config);
    assert_eq!(
        commands[0],
        Command::SetTimer {
            id: TimerId::Capture,
            after: Duration::from_secs(10),
        }
    );
    assert_eq!(records(&commands)[0].kind, HistoryKind::ConfigReload);
    assert_eq!(h.machine().config(), &config);
    assert_eq!(h.remaining(TimerId::Capture), Some(Duration::from_secs(10)));
}

#[test]
fn reload_outside_capture_states_only_records() {
    let mut h = reach(State::Prompting);
    let commands = h.apply_config(&Config::default());
    assert_eq!(commands.len(), 1);
    assert_eq!(records(&commands)[0].kind, HistoryKind::ConfigReload);
    assert_eq!(h.send(Event::ConfigReloaded), vec![]);
}

#[test]
fn reloads_reach_the_detector_in_every_state() {
    let reloaded = config(|c| c.stale.stale_percent = 55);
    for state in State::ALL {
        let mut h = reach(state);
        h.apply_config(&reloaded);
        assert_eq!(h.detector().configs(), vec![reloaded.clone()], "{state}");
    }
}

#[test]
fn a_threshold_reload_keeps_the_counters() {
    let mut h = reach(State::Monitoring);
    let resets = h.detector().resets();
    h.apply_config(&config(|c| c.stale.stale_percent = 55));
    assert_eq!(h.detector().resets(), resets);
    assert_eq!(h.state(), State::Monitoring);
    h.advance(Duration::from_secs(60));
    h.capture(true);
    assert_eq!(h.state(), State::Prompting);
}

#[test]
fn output_changes_reach_the_detector_in_every_state() {
    let outputs = vec![OutputInfo::new("HDMI-A-1", 3840, 2160)];
    for state in State::ALL {
        let mut h = reach(state);
        assert_eq!(h.send(Event::OutputsChanged(outputs.clone())), vec![]);
        assert_eq!(h.state(), state);
        assert_eq!(
            h.detector().output_updates(),
            vec![outputs.clone()],
            "{state}"
        );
    }
}

#[test]
fn output_changes_reach_the_detector_while_asleep() {
    let mut h = reach(State::Blanked);
    h.send(SessionEvent::PrepareForSleep);
    assert_eq!(h.send(Event::OutputsChanged(vec![])), vec![]);
    assert_eq!(h.detector().output_updates(), vec![vec![]]);
}

#[test]
fn a_replaced_detector_takes_over() {
    let clock = FakeClock::new();
    let old = ScriptedDetector::new();
    let (mut machine, _) =
        StateMachine::new(&Config::default(), Box::new(old.clone()), clock.now());
    let idle = Event::Activity(crate::event::ActivityEvent::InputIdle);
    machine.handle(clock.now(), clock.wall_now(), &idle);

    let fresh = ScriptedDetector::new();
    fresh.push_verdict(true);
    machine.replace_detector(Box::new(fresh.clone()));
    let done = Event::CaptureCompleted { frames: vec![] };
    machine.handle(clock.now(), clock.wall_now(), &done);
    assert_eq!(machine.state(), State::Prompting);
    assert_eq!(fresh.observations().len(), 1);
    assert_eq!(old.observations(), vec![]);
}

#[test]
fn stray_events_are_ignored() {
    let stray = [
        Event::CaptureCompleted { frames: vec![] },
        Event::CaptureFailed {
            error: crate::backend::BackendError::PermissionDenied("kwin".into()),
        },
        Event::PromptAnswered(PromptOutcome::Timeout),
        Event::Timer(TimerId::PromptCountdown),
        Event::Timer(TimerId::SnoozeExpiry),
        Event::DisplayPower {
            output: "HDMI-A-1".into(),
            on: false,
            kind: crate::event::PowerKind::Dpms,
        },
        Event::OutputsChanged(vec![]),
        Event::Session(SessionEvent::ResumedFromSleep),
        Event::Control(ControlCommand::Reload),
    ];
    for state in [State::Active, State::Blanked, State::Locked, State::Paused] {
        let mut h = reach(state);
        for event in stray.clone() {
            let commands = h.send(event.clone());
            if matches!(event, Event::DisplayPower { .. }) {
                assert!(
                    commands.iter().any(|command| matches!(
                        command,
                        Command::SetTimer {
                            id: TimerId::PanelCareReminder,
                            ..
                        }
                    )),
                    "{state}: {commands:?}"
                );
            } else {
                assert_eq!(commands, vec![], "{state}: {event:?}");
            }
        }
        assert_eq!(h.state(), state);
    }
}

#[test]
fn harness_fire_without_a_pending_timer_does_nothing() {
    let mut h = Harness::default();
    assert_eq!(h.fire(TimerId::Capture), vec![]);
    assert_eq!(h.status().in_state, Duration::ZERO);
}
