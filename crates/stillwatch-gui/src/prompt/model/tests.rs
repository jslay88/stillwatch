use stillwatch_core::config::PromptConfig;
use stillwatch_core::state::State;
use stillwatch_ipc::prompt::PromptAnswerKind;

use super::{Dialog, Input, Note, Step, update};

fn opened(custom: bool) -> Dialog {
    Dialog::new(&PromptConfig::default(), 3, custom)
}

fn answer(dialog: &mut Dialog, input: Input) -> (PromptAnswerKind, u32) {
    match update(dialog, input) {
        Step::Answer(kind, minutes) => (kind, minutes),
        other => panic!("expected an answer, got {other:?}"),
    }
}

#[test]
fn the_countdown_ticks_and_times_out_once() {
    let mut dialog = opened(false);
    assert_eq!(update(&mut dialog, Input::Tick), Step::Stay);
    assert_eq!(dialog.remaining_secs(), 2);
    assert_eq!(update(&mut dialog, Input::Tick), Step::Stay);
    assert_eq!(dialog.remaining_secs(), 1);
    assert_eq!(
        answer(&mut dialog, Input::Tick),
        (PromptAnswerKind::Timeout, 0)
    );
    assert_eq!(update(&mut dialog, Input::Tick), Step::Stay);
    assert_eq!(update(&mut dialog, Input::BlankNow), Step::Stay);
    assert_eq!(dialog.remaining_secs(), 0);
}

#[test]
fn a_zero_countdown_times_out_on_the_first_tick() {
    let mut dialog = Dialog::new(&PromptConfig::default(), 0, false);
    assert_eq!(
        answer(&mut dialog, Input::Tick),
        (PromptAnswerKind::Timeout, 0)
    );
}

#[test]
fn buttons_and_keys_map_onto_prompt_answers() {
    let cases = [
        (Input::Snooze(15), PromptAnswerKind::Snooze, 15),
        (Input::BlankNow, PromptAnswerKind::Timeout, 0),
        (Input::Cancel, PromptAnswerKind::Cancel, 0),
        (Input::Escape, PromptAnswerKind::Cancel, 0),
        (Input::Dismiss, PromptAnswerKind::Dismissed, 0),
        (Input::Enter, PromptAnswerKind::Snooze, 15),
    ];
    for (input, kind, minutes) in cases {
        let mut dialog = opened(false);
        assert_eq!(answer(&mut dialog, input), (kind, minutes));
    }
}

#[test]
fn enter_does_nothing_without_a_preset() {
    let mut config = PromptConfig::default();
    config.snooze_presets_minutes.clear();
    let mut dialog = Dialog::new(&config, 5, false);
    assert_eq!(update(&mut dialog, Input::Enter), Step::Stay);
    assert_eq!(dialog.remaining_secs(), 5);
}

#[test]
fn custom_durations_are_checked_against_the_bounds() {
    let mut dialog = opened(true);
    assert!(dialog.custom_open().is_some());
    assert_eq!(
        update(&mut dialog, Input::CustomText("45m".into())),
        Step::Stay
    );
    assert_eq!(
        answer(&mut dialog, Input::SubmitCustom),
        (PromptAnswerKind::Snooze, 45)
    );

    let mut dialog = opened(true);
    assert_eq!(
        update(&mut dialog, Input::CustomText("45".into())),
        Step::Stay
    );
    assert_eq!(
        answer(&mut dialog, Input::SubmitCustom),
        (PromptAnswerKind::Snooze, 45)
    );

    let mut dialog = opened(true);
    assert_eq!(
        update(&mut dialog, Input::CustomText("1h30m".into())),
        Step::Stay
    );
    assert_eq!(
        answer(&mut dialog, Input::SubmitCustom),
        (PromptAnswerKind::Snooze, 90)
    );

    for text in ["soon", "0m", "90s", "721m", ""] {
        let mut dialog = opened(true);
        let _ = update(&mut dialog, Input::CustomText(text.into()));
        assert_eq!(update(&mut dialog, Input::SubmitCustom), Step::Stay);
        let (_, error) = dialog.custom_open().expect("field stays open");
        assert!(error.is_some(), "{text}");
    }
}

#[test]
fn custom_is_hidden_until_asked_and_unavailable_when_disabled() {
    let mut dialog = opened(false);
    assert!(dialog.custom_open().is_none());
    assert!(dialog.custom_available());
    assert_eq!(update(&mut dialog, Input::ToggleCustom), Step::Stay);
    assert!(dialog.custom_open().is_some());
    assert_eq!(update(&mut dialog, Input::SubmitCustom), Step::Stay);

    let config = PromptConfig {
        allow_custom: false,
        ..PromptConfig::default()
    };
    let mut dialog = Dialog::new(&config, 5, true);
    assert!(!dialog.custom_available());
    assert_eq!(update(&mut dialog, Input::ToggleCustom), Step::Stay);
    assert!(!dialog.custom_available());
    assert_eq!(update(&mut dialog, Input::SubmitCustom), Step::Stay);
}

#[test]
fn leaving_prompting_closes_without_an_answer() {
    let mut dialog = opened(false);
    assert_eq!(
        update(&mut dialog, Input::State(State::Prompting)),
        Step::Stay
    );
    assert_eq!(
        update(&mut dialog, Input::State(State::Active)),
        Step::Close
    );
    assert_eq!(update(&mut dialog, Input::BlankNow), Step::Stay);

    let mut dialog = opened(false);
    let step = update(
        &mut dialog,
        Input::Noted(Note {
            state: State::Snoozed,
            summary: "HDMI-A-1 is 84% unchanged".into(),
            remaining_secs: Some(9),
        }),
    );
    assert_eq!(step, Step::Close);
    assert_eq!(dialog.summary(), "HDMI-A-1 is 84% unchanged");
}

#[test]
fn a_status_sets_the_summary_and_the_countdown_until_the_first_tick() {
    let mut dialog = opened(false);
    assert_eq!(
        update(
            &mut dialog,
            Input::Noted(Note {
                state: State::Prompting,
                summary: "DP-1 is 90% unchanged".into(),
                remaining_secs: Some(40),
            }),
        ),
        Step::Stay
    );
    assert_eq!(dialog.remaining_secs(), 40);
    assert_eq!(dialog.summary(), "DP-1 is 90% unchanged");
    assert_eq!(update(&mut dialog, Input::Tick), Step::Stay);
    assert_eq!(
        update(
            &mut dialog,
            Input::Noted(Note {
                state: State::Prompting,
                summary: "DP-1 is 91% unchanged".into(),
                remaining_secs: Some(1),
            }),
        ),
        Step::Stay
    );
    assert_eq!(dialog.remaining_secs(), 39);
    assert_eq!(dialog.summary(), "DP-1 is 91% unchanged");
}

#[test]
fn a_notice_is_shown_and_does_not_close_the_opened() {
    let mut dialog = opened(false);
    assert_eq!(
        update(
            &mut dialog,
            Input::Notice("stillwatchd is not running".into())
        ),
        Step::Stay
    );
    assert_eq!(dialog.notice(), Some("stillwatchd is not running"));
}
