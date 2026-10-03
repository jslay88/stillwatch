use super::super::catalog::{ACTIVITY_PULSE_SECONDS, GamepadSeen, activity_lit};
use super::{
    NOT_CONNECTED, gamepad_rows, output_rows, player_rows, player_value, set_exact, set_gamepad,
    set_player,
};

fn pad(name: &str, age: Option<u64>) -> GamepadSeen {
    GamepadSeen {
        id: format!("id-{name}"),
        name: name.to_owned(),
        seconds_since_activity: age,
    }
}

#[test]
fn output_rows_keep_configured_names_that_are_not_connected() {
    let rows = output_rows(
        &["HDMI-A-1".into(), "eDP-1".into()],
        &["HDMI-A-1".into(), "DP-1".into()],
    );
    assert_eq!(rows[0].value, "HDMI-A-1");
    assert!(rows[0].selected);
    assert!(rows[0].connected);
    assert_eq!(rows[1].value, "DP-1");
    assert!(!rows[1].selected);
    assert!(rows[1].connected);
    assert_eq!(rows[2].value, "eDP-1");
    assert!(rows[2].selected);
    assert!(!rows[2].connected);
    assert_eq!(rows[2].absence(), Some(NOT_CONNECTED));
}

#[test]
fn an_empty_output_list_selects_nothing() {
    let rows = output_rows(&[], &["HDMI-A-1".into()]);
    assert!(rows.iter().all(|row| !row.selected));
    assert!(rows.iter().all(|row| row.connected));
}

#[test]
fn configured_outputs_stay_editable_when_nothing_is_live() {
    let rows = output_rows(&["HDMI-A-1".into()], &[]);
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].connected);
    assert_eq!(
        set_exact(&["HDMI-A-1".into()], "DP-2", true),
        vec!["HDMI-A-1".to_owned(), "DP-2".to_owned()]
    );
}

#[test]
fn selecting_a_gamepad_stores_its_name_and_a_shorter_needle_already_counts() {
    let live = vec![pad("Drifty Pad", Some(0))];
    let rows = gamepad_rows(&["drifty".into()], &live);
    assert!(rows[0].selected);
    assert_eq!(rows[0].activity_label(), Some("active"));
    assert_eq!(
        set_gamepad(&["drifty".into()], "Drifty Pad", true),
        ["drifty"]
    );
    assert_eq!(
        set_gamepad(&[], "Drifty Pad", true),
        vec!["Drifty Pad".to_owned()]
    );
    assert_eq!(
        set_gamepad(&["drifty".into(), "other".into()], "Drifty Pad", false),
        ["other"]
    );
}

#[test]
fn a_missing_gamepad_stays_as_not_connected() {
    let rows = gamepad_rows(&["old pad".into()], &[pad("Other", None)]);
    assert_eq!(rows.len(), 2);
    assert!(!rows[1].connected);
    assert_eq!(rows[1].absence(), Some(NOT_CONNECTED));
    assert!(rows[1].activity_label().is_none());
}

#[test]
fn activity_pulses_only_inside_the_window() {
    assert!(activity_lit(Some(0)));
    assert!(activity_lit(Some(ACTIVITY_PULSE_SECONDS - 1)));
    assert!(!activity_lit(Some(ACTIVITY_PULSE_SECONDS)));
    assert!(!activity_lit(None));

    let pulsing = gamepad_rows(&[], &[pad("Pad", Some(ACTIVITY_PULSE_SECONDS - 1))]);
    let quiet = gamepad_rows(&[], &[pad("Pad", Some(ACTIVITY_PULSE_SECONDS))]);
    let never = gamepad_rows(&[], &[pad("Pad", None)]);
    assert_eq!(pulsing[0].activity, Some(true));
    assert_eq!(quiet[0].activity, Some(false));
    assert_eq!(never[0].activity, Some(false));
    assert!(quiet[0].activity_label().is_none());
}

#[test]
fn player_rows_store_the_stem_and_keep_absent_names() {
    assert_eq!(player_value("firefox.instance_1_42"), "firefox");
    assert_eq!(player_value("vlc.instance7389"), "vlc");
    assert_eq!(player_value("spotify"), "spotify");

    let live = vec![
        "firefox.instance_1_42".into(),
        "firefox.instance_9".into(),
        "spotify".into(),
    ];
    let rows = player_rows(&["firefox".into(), "vlc".into()], &live);
    assert_eq!(rows[0].value, "firefox");
    assert!(rows[0].selected);
    assert!(rows[0].connected);
    assert_eq!(rows[1].value, "spotify");
    assert!(!rows[1].selected);
    assert_eq!(rows[2].value, "vlc");
    assert!(!rows[2].connected);

    assert_eq!(
        set_player(&["vlc".into()], "firefox", &live, true),
        vec!["vlc".to_owned(), "firefox".to_owned()]
    );
    assert_eq!(
        set_player(&["firefox".into(), "vlc".into()], "firefox", &live, false),
        vec!["vlc".to_owned()]
    );
}

#[test]
fn an_old_player_instance_name_still_selects_the_live_stem() {
    let live = ["firefox.instance_1_42".to_owned()];
    let rows = player_rows(&["firefox.instance_1_42".into()], &live);
    assert_eq!(rows.len(), 1);
    assert!(rows[0].selected);
    assert_eq!(rows[0].value, "firefox");
    assert_eq!(
        set_player(&["firefox.instance_1_42".into()], "firefox", &live, false),
        Vec::<String>::new()
    );
}
