//! Output hotplug and a lost compositor idle watch.

use std::time::Duration;

use super::{config, records_of};
use crate::command::Command;
use crate::config::WhenLocked;
use crate::event::{ActivityEvent, Event, SessionEvent};
use crate::history::HistoryKind;
use crate::luma::OutputInfo;
use crate::mocks::Harness;
use crate::state::State;
use crate::time::TimerId;

fn hdmi(generation: u64) -> OutputInfo {
    OutputInfo::new("HDMI-A-1", 1920, 1080).with_generation(generation)
}

fn dp(generation: u64) -> OutputInfo {
    OutputInfo::new("DP-1", 1920, 1080).with_generation(generation)
}

fn outputs(list: Vec<OutputInfo>) -> Event {
    Event::OutputsChanged(list)
}

fn hotplug(commands: &[Command]) -> Vec<(String, u32)> {
    records_of(commands, HistoryKind::Hotplug)
        .into_iter()
        .filter_map(|entry| Some((entry.output?, entry.count?)))
        .collect()
}

fn has_timer(commands: &[Command], id: TimerId) -> bool {
    commands
        .iter()
        .any(|command| matches!(command, Command::SetTimer { id: armed, .. } if *armed == id))
}

#[test]
fn the_first_output_list_is_not_history() {
    let mut h = Harness::new();
    let commands = h.send(outputs(vec![hdmi(0)]));
    assert_eq!(hotplug(&commands), []);
    assert_eq!(h.state(), State::Active);
}

#[test]
fn adding_and_removing_outputs_records_names_only() {
    let mut h = Harness::new();
    h.send(outputs(vec![hdmi(0)]));
    let added = h.send(outputs(vec![hdmi(0), dp(0)]));
    assert_eq!(hotplug(&added), [("DP-1".into(), 1)]);
    let removed = h.send(outputs(vec![hdmi(0)]));
    assert_eq!(hotplug(&removed), [("DP-1".into(), 0)]);
}

#[test]
fn a_reused_connector_name_is_a_removal_and_an_addition() {
    let mut h = Harness::new();
    h.send(outputs(vec![hdmi(0)]));
    let reused = h.send(outputs(vec![hdmi(1)]));
    assert_eq!(
        hotplug(&reused),
        [("HDMI-A-1".into(), 0), ("HDMI-A-1".into(), 1)]
    );
}

#[test]
fn unknown_activity_leaves_monitoring_without_blanking() {
    let mut h = Harness::new();
    h.idle();
    assert_eq!(h.state(), State::Monitoring);
    let commands = h.send(ActivityEvent::Unknown);
    assert_eq!(h.state(), State::Active);
    let reconnect = records_of(&commands, HistoryKind::Reconnect);
    assert_eq!(reconnect.len(), 1);
    assert_eq!(reconnect[0].count, Some(1));
    assert!(reconnect[0].output.is_none());
    assert!(!commands.iter().any(|command| matches!(
        command,
        Command::Blank { .. } | Command::RequestCapture { .. }
    )));
}

#[test]
fn unknown_activity_ends_a_prompt_without_acting() {
    let mut h = Harness::new();
    h.to_prompting();
    let commands = h.send(ActivityEvent::Unknown);
    assert_eq!(h.state(), State::Active);
    assert!(
        commands
            .iter()
            .any(|command| matches!(command, Command::DismissPrompt))
    );
    assert!(
        !commands
            .iter()
            .any(|command| matches!(command, Command::Blank { .. }))
    );
}

#[test]
fn unknown_activity_while_blanked_does_not_wake_or_reblank() {
    let mut h = Harness::new();
    h.to_blanked();
    let lost = h.send(ActivityEvent::Unknown);
    assert_eq!(h.state(), State::Blanked);
    assert!(
        !lost
            .iter()
            .any(|command| matches!(command, Command::Unblank { .. }))
    );
    let power = h.display_on();
    assert!(!has_timer(&power, TimerId::ReblankGrace));
}

#[test]
fn unknown_activity_while_locked_does_not_blank() {
    let mut h = Harness::with_config(&config(|cfg| {
        cfg.session.when_locked = WhenLocked::BlankAfter;
        cfg.session.locked_blank_seconds = 60;
    }));
    h.send(Event::Session(SessionEvent::Locked));
    assert_eq!(h.state(), State::Locked);
    h.send(ActivityEvent::Unknown);
    let waited = h.advance(Duration::from_secs(60));
    assert_eq!(h.state(), State::Locked);
    assert!(
        !waited
            .iter()
            .any(|command| matches!(command, Command::Blank { .. }))
    );
    let rearmed = h.send(ActivityEvent::InputIdle);
    assert!(has_timer(&rearmed, TimerId::LockedBlank));
    assert_eq!(h.state(), State::Locked);
}

#[test]
fn no_monitored_outputs_pauses_capture_and_does_not_reblank() {
    let mut h = Harness::with_config(&config(|cfg| cfg.stale.check_interval_seconds = 5));
    h.idle();
    h.send(outputs(vec![hdmi(0)]));
    let gone = h.send(outputs(Vec::new()));
    assert_eq!(hotplug(&gone), [("HDMI-A-1".into(), 0)]);
    assert!(
        gone.iter()
            .any(|command| matches!(command, Command::CancelTimer(TimerId::Capture)))
    );
    let waited = h.advance(Duration::from_secs(5));
    assert!(
        !waited
            .iter()
            .any(|command| matches!(command, Command::RequestCapture { .. }))
    );

    h.send(ActivityEvent::InputResumed);
    h.to_blanked();
    h.send(outputs(vec![hdmi(0)]));
    h.send(outputs(Vec::new()));
    assert!(!has_timer(&h.display_on(), TimerId::ReblankGrace));
}

#[test]
fn a_replaced_output_reblanks_once_until_the_next_blank() {
    let mut h = Harness::new();
    h.send(outputs(vec![hdmi(0)]));
    h.to_blanked();
    h.send(outputs(vec![hdmi(1)]));
    assert!(has_timer(&h.display_on(), TimerId::ReblankGrace));
    assert!(!has_timer(&h.display_on(), TimerId::ReblankGrace));

    h.advance(Duration::from_secs(15));
    h.send(Event::ActionCompleted);
    assert_eq!(h.state(), State::Blanked);
    assert!(
        has_timer(&h.display_on(), TimerId::ReblankGrace),
        "the same generation after a fresh blank is a normal wake"
    );
}

#[test]
fn an_output_coming_back_resumes_capture() {
    let mut h = Harness::with_config(&config(|cfg| cfg.stale.check_interval_seconds = 30));
    h.idle();
    h.send(outputs(vec![hdmi(0)]));
    h.send(outputs(Vec::new()));
    let back = h.send(outputs(vec![hdmi(2)]));
    assert_eq!(hotplug(&back), [("HDMI-A-1".into(), 1)]);
    assert!(
        back.iter()
            .any(|command| matches!(command, Command::RequestCapture { .. }))
    );
    assert!(has_timer(&back, TimerId::Capture));
}
