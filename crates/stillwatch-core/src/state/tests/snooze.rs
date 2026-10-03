//! Snooze duration validation.

use std::time::Duration;

use crate::command::Command;
use crate::config::{Config, PromptConfig};
use crate::event::ControlCommand;
use crate::mocks::Harness;
use crate::prompt::PromptOutcome;
use crate::state::{SnoozeError, State, validate_snooze};
use crate::time::TimerId;

fn mins(m: u64) -> Duration {
    Duration::from_mins(m)
}

#[test]
fn presets_are_accepted() {
    let prompt = PromptConfig::default();
    for preset in [15, 60, 180] {
        assert_eq!(validate_snooze(&prompt, mins(preset)), Ok(mins(preset)));
    }
}

#[test]
fn custom_durations_must_be_within_bounds() {
    let prompt = PromptConfig::default();
    assert_eq!(validate_snooze(&prompt, mins(1)), Ok(mins(1)));
    assert_eq!(validate_snooze(&prompt, mins(45)), Ok(mins(45)));
    assert_eq!(validate_snooze(&prompt, mins(720)), Ok(mins(720)));
    assert_eq!(
        validate_snooze(&prompt, Duration::from_secs(59)),
        Err(SnoozeError::TooShort { min_minutes: 1 })
    );
    let too_long = validate_snooze(&prompt, mins(720) + Duration::from_secs(1));
    assert_eq!(too_long, Err(SnoozeError::TooLong { max_minutes: 720 }));
    assert_eq!(
        too_long.unwrap_err().to_string(),
        "snooze must be at most 720 minutes"
    );
}

#[test]
fn custom_durations_are_refused_when_custom_is_off() {
    let prompt = PromptConfig {
        allow_custom: false,
        ..PromptConfig::default()
    };
    assert_eq!(validate_snooze(&prompt, mins(60)), Ok(mins(60)));
    let err = validate_snooze(&prompt, mins(45)).unwrap_err();
    assert_eq!(
        err,
        SnoozeError::CustomDisabled {
            presets: vec![15, 60, 180]
        }
    );
    assert_eq!(
        err.to_string(),
        "custom snooze durations are disabled, pick a preset ([15, 60, 180] minutes)"
    );
    assert_eq!(
        SnoozeError::TooShort { min_minutes: 5 }.to_string(),
        "snooze must be at least 5 minutes"
    );
}

#[test]
fn the_machine_validates_against_its_current_config() {
    let mut h = Harness::new();
    assert_eq!(h.machine().validate_snooze(mins(45)), Ok(mins(45)));
    let mut config = Config::default();
    config.prompt.allow_custom = false;
    h.apply_config(&config);
    assert!(h.machine().validate_snooze(mins(45)).is_err());
}

#[test]
fn an_invalid_snooze_answer_keeps_prompting() {
    let mut h = Harness::new();
    h.to_prompting();
    let commands = h.answer(PromptOutcome::Snooze(mins(721)));
    assert_eq!(commands.len(), 1);
    assert!(matches!(&commands[0], Command::Record(e) if e.snooze_seconds == Some(43_260)));
    assert_eq!(h.send(ControlCommand::Snooze(Duration::ZERO)), vec![]);
    assert_eq!(h.state(), State::Prompting);
    h.fire(TimerId::PromptCountdown);
    assert_eq!(h.state(), State::Acting);
}

#[test]
fn an_invalid_snooze_command_is_ignored_everywhere() {
    let mut h = Harness::new();
    assert_eq!(h.send(ControlCommand::Snooze(mins(721))), vec![]);
    h.send(ControlCommand::Snooze(mins(15)));
    assert_eq!(h.send(ControlCommand::Snooze(mins(721))), vec![]);
    assert_eq!(h.remaining(TimerId::SnoozeExpiry), Some(mins(15)));
}
