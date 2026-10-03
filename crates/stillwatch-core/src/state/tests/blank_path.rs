//! The full path to blanked displays, and the action modes.

use std::time::Duration;

use super::{changed, effects, record_kinds, records};
use crate::command::{BlankMethod, Command, HookKind};
use crate::config::{ActionMode, ActionOutputs, Config};
use crate::event::Event;
use crate::history::{HistoryKind, PromptAnswer};
use crate::mocks::Harness;
use crate::state::State;
use crate::time::TimerId;

fn config(edit: impl FnOnce(&mut Config)) -> Config {
    let mut config = Config::default();
    edit(&mut config);
    config
}

#[test]
fn desktop_blanking_first() {
    let mut h = Harness::new();
    h.idle();
    for _ in 0..4 {
        h.detector().push_verdict(false);
    }
    h.detector().push_verdict(true);

    // The first capture is requested on entry; ticks follow every minute.
    let mut ticks = 0;
    loop {
        h.complete_capture();
        ticks += 1;
        if h.state() != State::Monitoring {
            break;
        }
        h.advance(Duration::from_mins(1));
    }
    assert_eq!(ticks, 5);
    assert_eq!(h.state(), State::Prompting);
    assert_eq!(h.status().in_state, Duration::ZERO);

    h.advance(Duration::from_mins(1));
    assert_eq!(h.state(), State::Acting);
    h.send(Event::ActionCompleted);

    assert_eq!(
        h.transitions(),
        vec![
            (State::Active, State::Monitoring),
            (State::Monitoring, State::Prompting),
            (State::Prompting, State::Acting),
            (State::Acting, State::Blanked),
        ]
    );
    let entries = records(h.log());
    assert_eq!(
        record_kinds(h.log()),
        [
            HistoryKind::Transition,
            HistoryKind::Transition,
            HistoryKind::Prompt,
            HistoryKind::PromptAnswered,
            HistoryKind::Transition,
            HistoryKind::Blank,
            HistoryKind::Transition,
        ]
    );
    assert!(entries[1].detection.as_ref().is_some_and(|d| d.stale));
    assert_eq!(entries[2].detection, entries[1].detection);
    assert_eq!(entries[3].answer, Some(PromptAnswer::Timeout));
    assert_eq!(entries[5].blank_method, Some(BlankMethod::Dpms));
    assert_eq!(entries[6].blank_method, Some(BlankMethod::Dpms));
    assert!(h.timers().is_empty());
}

#[test]
fn blank_targets_the_configured_outputs() {
    let monitored = config(|c| {
        c.stale.monitored_outputs = vec!["HDMI-A-1".into()];
        c.action.blank_method = BlankMethod::Overlay;
    });
    let mut h = Harness::with_config(&monitored);
    let commands = h.to_blanked();
    let blank = Command::Blank {
        outputs: vec!["HDMI-A-1".into()],
        method: BlankMethod::Overlay,
    };
    assert!(commands.contains(&blank));
    assert!(commands.contains(&Command::RequestCapture {
        outputs: vec!["HDMI-A-1".into()],
        downscale_width: 480,
    }));
    let commands = h.input();
    assert_eq!(
        commands[0],
        Command::Unblank {
            outputs: vec!["HDMI-A-1".into()]
        }
    );

    let all = config(|c| {
        c.stale.monitored_outputs = vec!["HDMI-A-1".into()];
        c.action.outputs = ActionOutputs::All;
    });
    let mut h = Harness::with_config(&all);
    let commands = h.to_blanked();
    assert!(commands.contains(&Command::Blank {
        outputs: vec![],
        method: BlankMethod::Dpms,
    }));
}

#[test]
fn hooks_run_around_blanking_when_configured() {
    let hooks = config(|c| {
        c.action.on_blank_cmd = "tv-off".into();
        c.action.on_resume_cmd = "tv-on".into();
    });
    let mut h = Harness::with_config(&hooks);
    let commands = h.to_blanked();
    assert_eq!(commands.last(), Some(&Command::RunHook(HookKind::OnBlank)));
    let commands = h.input();
    assert_eq!(
        commands[..2],
        [
            Command::Unblank { outputs: vec![] },
            Command::RunHook(HookKind::OnResume)
        ]
    );
}

#[test]
fn lock_and_blank_locks_first() {
    let mut h = Harness::with_config(&config(|c| c.action.mode = ActionMode::LockAndBlank));
    h.to_prompting();
    let commands = h.fire(TimerId::PromptCountdown);
    assert_eq!(commands.last(), Some(&Command::Lock));
    let commands = h.send(Event::ActionCompleted);
    assert_eq!(
        effects(&commands),
        vec![Command::Blank {
            outputs: vec![],
            method: BlankMethod::Dpms,
        }]
    );
    assert_eq!(record_kinds(&commands), [HistoryKind::Blank]);
    let commands = h.send(Event::ActionCompleted);
    assert_eq!(commands[0], changed(State::Acting, State::Blanked));
}

#[test]
fn dim_then_blank_still_sends_blank() {
    // The machine emits Blank; ActionRunner dims first, then blanks.
    let mut h = Harness::with_config(&config(|c| c.action.mode = ActionMode::DimThenBlank));
    let commands = h.to_blanked();
    assert!(commands.contains(&Command::Blank {
        outputs: vec![],
        method: BlankMethod::Dpms,
    }));
    assert_eq!(h.state(), State::Blanked);
}

#[test]
fn command_mode_runs_the_command_and_counts_as_blanked() {
    let command = config(|c| {
        c.action.mode = ActionMode::Command;
        c.action.command = "my-screen-off".into();
    });
    let mut h = Harness::with_config(&command);
    h.to_prompting();
    let commands = h.fire(TimerId::PromptCountdown);
    assert!(commands.contains(&Command::RunHook(HookKind::ActionCommand)));
    assert_eq!(
        h.transitions()[2..],
        [
            (State::Prompting, State::Acting),
            (State::Acting, State::Blanked)
        ]
    );
    assert!(!h.machine().displays_blanked());
    let commands = h.input();
    assert_eq!(commands[0], changed(State::Blanked, State::Active));
    assert!(
        !commands
            .iter()
            .any(|c| matches!(c, Command::Unblank { .. }))
    );
}

#[test]
fn late_action_results_are_ignored() {
    let mut h = Harness::new();
    h.to_blanked();
    assert_eq!(h.send(Event::ActionCompleted), vec![]);
    h.input();
    assert_eq!(h.send(Event::ActionCompleted), vec![]);
    assert_eq!(h.state(), State::Active);
}
