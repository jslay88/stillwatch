use serde::Serialize;
use toml::Table;

use super::test_support::fixture;
use super::*;

const SECTIONS: [&str; 11] = [
    "idle",
    "session",
    "activity",
    "capture",
    "stale",
    "safety",
    "prompt",
    "action",
    "panel_care",
    "history",
    "logging",
];

fn parse(input: &str) -> Result<Config, ConfigError> {
    Config::from_toml_str(input).map(|outcome| outcome.config)
}

fn parse_error_key(input: &str) -> String {
    match parse(input) {
        Err(ConfigError::Parse { key, .. }) => key,
        other => panic!("expected a parse error for {input:?}, got {other:?}"),
    }
}

fn names<T: Serialize>(variants: &[T]) -> Vec<String> {
    variants
        .iter()
        .map(|v| {
            toml::Value::try_from(v)
                .unwrap()
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect()
}

#[test]
fn default_serializes_to_the_documented_config() {
    let documented: Table = toml::from_str(&fixture("default.toml")).unwrap();
    let serialized: Table = toml::from_str(&Config::default().to_toml_string().unwrap()).unwrap();
    assert_eq!(serialized, documented);
}

#[test]
fn documented_config_loads_as_default() {
    let outcome = Config::from_toml_str(&fixture("default.toml")).unwrap();
    assert_eq!(outcome.config, Config::default());
    assert_eq!(outcome.migrated_from, None);
    assert_eq!(outcome.notes, []);
}

#[test]
fn default_is_valid() {
    assert_eq!(Config::default().validate(), Ok(()));
}

#[test]
fn empty_document_is_default() {
    assert_eq!(parse("").unwrap(), Config::default());
}

#[test]
fn every_section_is_written() {
    let written: Table = toml::from_str(&Config::default().to_toml_string().unwrap()).unwrap();
    for section in SECTIONS {
        assert!(written[section].is_table(), "{section} missing");
    }
}

#[test]
fn to_toml_string_round_trips_ignore_regions() {
    let mut config = Config::default();
    config.stale.ignore_regions = vec![
        IgnoreRegion {
            output: "HDMI-A-1".to_owned(),
            x: 0,
            y: 0,
            w: 200,
            h: 40,
        },
        IgnoreRegion {
            output: "DP-2".to_owned(),
            x: 3640,
            y: 2120,
            w: 200,
            h: 40,
        },
    ];
    assert_eq!(parse(&config.to_toml_string().unwrap()).unwrap(), config);
}

#[test]
fn single_section_leaves_others_default() {
    let config = parse(&fixture("partial_stale.toml")).unwrap();
    let mut expected = Config::default();
    expected.stale.stale_percent = 80;
    expected.stale.monitored_outputs = vec!["HDMI-A-1".to_owned()];
    expected.stale.ignore_regions = vec![IgnoreRegion {
        output: "HDMI-A-1".to_owned(),
        x: 0,
        y: 0,
        w: 200,
        h: 40,
    }];
    assert_eq!(config, expected);
}

#[test]
fn single_key_leaves_rest_of_section_default() {
    let config = parse("[prompt]\ncountdown_seconds = 30\n").unwrap();
    let expected = PromptConfig {
        countdown_seconds: 30,
        ..PromptConfig::default()
    };
    assert_eq!(config.prompt, expected);
}

#[test]
fn non_default_variants_parse() {
    let config = parse(
        "[session]\nwhen_locked = \"pause\"\n\
         [capture]\nbackend = \"portal\"\n\
         [stale]\nrequire = \"any\"\n\
         [prompt]\nstyle = \"dialog\"\nurgency = \"low\"\n\
         [action]\nmode = \"dim_then_blank\"\nblank_method = \"ddc_standby\"\n\
         outputs = \"all\"\ndim_method = \"brightness\"\nreblank_fallback = \"none\"\n\
         [logging]\nlevel = \"trace\"\n",
    )
    .unwrap();
    assert_eq!(config.session.when_locked, WhenLocked::Pause);
    assert_eq!(config.capture.backend, CaptureBackend::Portal);
    assert_eq!(config.stale.require, StaleRequire::Any);
    assert_eq!(config.prompt.style, PromptStyle::Dialog);
    assert_eq!(config.prompt.urgency, PromptUrgency::Low);
    assert_eq!(config.action.mode, ActionMode::DimThenBlank);
    assert_eq!(config.action.blank_method, BlankMethod::DdcStandby);
    assert_eq!(config.action.outputs, ActionOutputs::All);
    assert_eq!(config.action.dim_method, DimMethod::Brightness);
    assert_eq!(config.action.reblank_fallback, ReblankFallback::None);
    assert_eq!(config.logging.level, LogLevel::Trace);
}

#[test]
fn enum_variants_use_documented_names() {
    use ActionMode as M;
    use LogLevel as L;
    assert_eq!(
        names(&[WhenLocked::Pause, WhenLocked::BlankAfter]),
        ["pause", "blank_after"]
    );
    assert_eq!(
        names(&[
            CaptureBackend::Auto,
            CaptureBackend::Kwin,
            CaptureBackend::Portal
        ]),
        ["auto", "kwin", "portal"]
    );
    assert_eq!(
        names(&[StaleRequire::All, StaleRequire::Any]),
        ["all", "any"]
    );
    assert_eq!(
        names(&[
            PromptStyle::Auto,
            PromptStyle::Notification,
            PromptStyle::Dialog
        ]),
        ["auto", "notification", "dialog"]
    );
    assert_eq!(
        names(&[
            PromptUrgency::Low,
            PromptUrgency::Normal,
            PromptUrgency::Critical
        ]),
        ["low", "normal", "critical"]
    );
    assert_eq!(
        names(&[M::Blank, M::LockAndBlank, M::DimThenBlank, M::Command]),
        ["blank", "lock_and_blank", "dim_then_blank", "command"]
    );
    assert_eq!(
        names(&[
            BlankMethod::Dpms,
            BlankMethod::Overlay,
            BlankMethod::DdcStandby
        ]),
        ["dpms", "overlay", "ddc_standby"]
    );
    assert_eq!(
        names(&[ActionOutputs::Monitored, ActionOutputs::All]),
        ["monitored", "all"]
    );
    assert_eq!(
        names(&[DimMethod::Overlay, DimMethod::Brightness]),
        ["overlay", "brightness"]
    );
    assert_eq!(
        names(&[ReblankFallback::Overlay, ReblankFallback::None]),
        ["overlay", "none"]
    );
    assert_eq!(
        names(&[L::Error, L::Warn, L::Info, L::Debug, L::Trace]),
        ["error", "warn", "info", "debug", "trace"]
    );
}

#[test]
fn unknown_key_in_any_section_is_rejected_with_its_path() {
    for section in SECTIONS {
        let key = parse_error_key(&format!("[{section}]\nbogus = 1\n"));
        assert_eq!(key, format!("{section}.bogus"));
    }
}

#[test]
fn unknown_top_level_key_is_rejected() {
    assert_eq!(parse_error_key("bogus = 1\n"), "bogus");
    assert_eq!(parse_error_key("[bogus]\nx = 1\n"), "bogus");
}

#[test]
fn typo_is_rejected_with_message() {
    let error = parse("[stale]\nstale_precent = 70\n").unwrap_err();
    let ConfigError::Parse { key, message } = &error else {
        panic!("expected parse error, got {error:?}");
    };
    assert_eq!(key, "stale.stale_precent");
    assert!(
        message.contains("unknown field `stale_precent`"),
        "{message}"
    );
    assert!(
        error
            .to_string()
            .starts_with("invalid config at `stale.stale_precent`")
    );
}

#[test]
fn unknown_enum_value_is_rejected_with_its_path() {
    let keys = [
        "session.when_locked",
        "capture.backend",
        "stale.require",
        "prompt.style",
        "prompt.urgency",
        "action.mode",
        "action.blank_method",
        "action.outputs",
        "action.dim_method",
        "action.reblank_fallback",
        "logging.level",
    ];
    for path in keys {
        let (section, key) = path.split_once('.').unwrap();
        let input = format!("[{section}]\n{key} = \"bogus\"\n");
        assert_eq!(parse_error_key(&input), path);
    }
}

#[test]
fn wrong_types_are_rejected_with_their_paths() {
    let cases = [
        (
            "[idle]\ninput_idle_minutes = \"ten\"\n",
            "idle.input_idle_minutes",
        ),
        (
            "[idle]\ninput_idle_minutes = -1\n",
            "idle.input_idle_minutes",
        ),
        ("[activity]\ngamepad = 1\n", "activity.gamepad"),
        ("[stale]\nblock_grid = [16]\n", "stale.block_grid"),
        ("[stale]\nblock_grid = [16, -1]\n", "stale.block_grid[1]"),
        (
            "[prompt]\nsnooze_presets_minutes = [15, \"x\"]\n",
            "prompt.snooze_presets_minutes[1]",
        ),
        ("idle = 3\n", "idle"),
    ];
    for (input, path) in cases {
        assert_eq!(parse_error_key(input), path, "{input}");
    }
}

#[test]
fn ignore_region_fields_are_required_and_strict() {
    let missing = "[stale]\nignore_regions = [{ output = \"A\", x = 0, y = 0, w = 1 }]\n";
    assert_eq!(parse_error_key(missing), "stale.ignore_regions[0]");
    let extra =
        "[stale]\nignore_regions = [{ output = \"A\", x = 0, y = 0, w = 1, h = 1, z = 2 }]\n";
    assert_eq!(parse_error_key(extra), "stale.ignore_regions[0].z");
}

#[test]
fn normal_path_seconds_does_not_overflow() {
    let stale = StaleConfig {
        persist_checks: u32::MAX,
        check_interval_seconds: u32::MAX,
        ..StaleConfig::default()
    };
    assert_eq!(
        stale.normal_path_seconds(),
        u64::from(u32::MAX) * u64::from(u32::MAX)
    );
}

#[test]
fn media_ignore_matches_players_and_their_instances() {
    let stale = StaleConfig {
        media_ignore_players: vec!["spotify".into(), "firefox".into()],
        ..StaleConfig::default()
    };
    assert!(stale.is_player_ignored("spotify"));
    assert!(stale.is_player_ignored("Spotify"));
    assert!(stale.is_player_ignored("firefox.instance_1_42"));
    assert!(!stale.is_player_ignored("firefoxdev"));
    assert!(!stale.is_player_ignored("vlc"));
    assert!(!stale.is_player_ignored("spot"));
    assert!(!stale.media_playing(&[]));
    assert!(!stale.media_playing(&["spotify".into()]));
    assert!(stale.media_playing(&["spotify".into(), "mpv".into()]));
}

#[test]
fn keyed_issues_cover_parse_and_validation_errors() {
    let parse_error = Config::from_toml_str("[stale]\nrequire = \"most\"\n").unwrap_err();
    let issues = parse_error.keyed_issues();
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].key, "stale.require");

    let invalid = Config::from_toml_str("[stale]\nstale_percent = 0\n[history]\nmax_entries = 0\n")
        .unwrap_err();
    let keys: Vec<_> = invalid.keyed_issues().into_iter().map(|i| i.key).collect();
    assert_eq!(keys, ["stale.stale_percent", "history.max_entries"]);

    let too_new = Config::from_toml_str("version = 99\n").unwrap_err();
    assert_eq!(too_new.keyed_issues(), []);
}

#[test]
fn hook_command_reads_each_script() {
    use crate::command::HookKind;

    let mut config = Config::default();
    config.action.on_blank_cmd = "blank".into();
    config.action.on_resume_cmd = "resume".into();
    config.action.command = "cmd".into();
    config.panel_care.trigger_cmd = "care".into();
    assert_eq!(config.hook_command(HookKind::OnBlank), "blank");
    assert_eq!(config.hook_command(HookKind::OnResume), "resume");
    assert_eq!(config.hook_command(HookKind::ActionCommand), "cmd");
    assert_eq!(config.hook_command(HookKind::PanelCareTrigger), "care");
}
