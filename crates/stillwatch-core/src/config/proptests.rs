use std::fmt::Debug;
use std::ops::RangeInclusive;

use proptest::collection::vec;
use proptest::prelude::*;
use proptest::sample::select;
use proptest::strategy::Union;

use super::test_support::{RANGE_RULES, issue_keys};
use super::*;

type Setter = fn(&mut Config, u32);

fn one_of<T: Clone + Debug + 'static>(values: &'static [T]) -> impl Strategy<Value = T> {
    select(values)
}

fn name() -> impl Strategy<Value = String> {
    "[A-Za-z0-9_.-]{1,12}"
}

fn text() -> impl Strategy<Value = String> {
    "\\PC{0,24}"
}

fn activity() -> impl Strategy<Value = ActivityConfig> {
    (any::<bool>(), 0..=100u32, vec(name(), 0..4), any::<bool>()).prop_map(
        |(gamepad, gamepad_deadzone_percent, gamepad_ignore_devices, gamepad_wakes_display)| {
            ActivityConfig {
                gamepad,
                gamepad_deadzone_percent,
                gamepad_ignore_devices,
                gamepad_wakes_display,
            }
        },
    )
}

fn region() -> impl Strategy<Value = IgnoreRegion> {
    (
        name(),
        any::<u32>(),
        any::<u32>(),
        1..=u32::MAX,
        1..=u32::MAX,
    )
        .prop_map(|(output, x, y, w, h)| IgnoreRegion { output, x, y, w, h })
}

fn stale() -> impl Strategy<Value = StaleConfig> {
    let detector = (
        1..=3600u32,
        1..=50u32,
        1..=100u32,
        0..=100u32,
        0..=255u32,
        0..=255u32,
    );
    let scope = (
        vec(name(), 0..3),
        vec(name(), 0..3),
        one_of(&[StaleRequire::All, StaleRequire::Any]),
        vec(region(), 0..3),
    );
    let grid = (16..=4096u32).prop_flat_map(|width| (Just(width), 1..=width, 1..=width));
    (detector, scope, grid).prop_map(
        |(
            (interval, persist, stale, media, luma, dark),
            (players, outputs, require, regions),
            (width, cols, rows),
        )| StaleConfig {
            check_interval_seconds: interval,
            persist_checks: persist,
            stale_percent: stale,
            media_stale_percent: media,
            media_ignore_players: players,
            luma_delta_threshold: luma,
            ignore_dark_below: dark,
            downscale_width: width,
            block_grid: [cols, rows],
            monitored_outputs: outputs,
            require,
            ignore_regions: regions,
        },
    )
}

fn prompt() -> impl Strategy<Value = PromptConfig> {
    let bounds = (any::<bool>(), 1..=120u32).prop_flat_map(|(allow_custom, min)| {
        let presets = (min..=2000).prop_flat_map(move |max| {
            let at_least = usize::from(!allow_custom);
            (Just(max), vec(min..=max, at_least..5))
        });
        (Just(allow_custom), Just(min), presets)
    });
    let style = one_of(&[
        PromptStyle::Auto,
        PromptStyle::Notification,
        PromptStyle::Dialog,
    ]);
    let urgency = one_of(&[
        PromptUrgency::Low,
        PromptUrgency::Normal,
        PromptUrgency::Critical,
    ]);
    let flags = (any::<bool>(), 1..=600u32, 1..=120u32, any::<bool>());
    (style, urgency, flags, bounds).prop_map(
        |(
            style,
            urgency,
            (fallback, countdown, grace, cancel),
            (allow_custom, min, (max, presets)),
        )| {
            PromptConfig {
                style,
                urgency,
                fallback_to_dialog: fallback,
                countdown_seconds: countdown,
                answer_grace_seconds: grace,
                snooze_presets_minutes: presets,
                allow_custom,
                custom_min_minutes: min,
                custom_max_minutes: max,
                snooze_cancelled_by_input: cancel,
            }
        },
    )
}

fn action() -> impl Strategy<Value = ActionConfig> {
    use ActionMode as M;
    let methods = (
        one_of(&[M::Blank, M::LockAndBlank, M::DimThenBlank, M::Command]),
        one_of(&[
            BlankMethod::Dpms,
            BlankMethod::Overlay,
            BlankMethod::DdcStandby,
        ]),
        one_of(&[ActionOutputs::Monitored, ActionOutputs::All]),
        one_of(&[DimMethod::Overlay, DimMethod::Brightness]),
        one_of(&[ReblankFallback::Overlay, ReblankFallback::None]),
    );
    let numbers = (0..=100u32, any::<u32>(), any::<u32>(), any::<u32>());
    let hooks = (name(), text(), text(), any::<bool>());
    (methods, numbers, hooks).prop_map(
        |(
            (mode, blank_method, outputs, dim_method, reblank_fallback),
            (dim_percent, dim_seconds, reblank_grace_seconds, reblank_max_attempts),
            (command, on_blank_cmd, on_resume_cmd, reblank_on_wake),
        )| ActionConfig {
            mode,
            blank_method,
            outputs,
            dim_method,
            dim_percent,
            dim_seconds,
            command,
            on_blank_cmd,
            on_resume_cmd,
            reblank_on_wake,
            reblank_grace_seconds,
            reblank_max_attempts,
            reblank_fallback,
        },
    )
}

fn arb_config() -> impl Strategy<Value = Config> {
    let input = (
        1..=10_000u32,
        one_of(&[WhenLocked::Pause, WhenLocked::BlankAfter]),
        any::<u32>(),
        activity(),
        one_of(&[
            CaptureBackend::Auto,
            CaptureBackend::Kwin,
            CaptureBackend::Portal,
        ]),
    );
    let safety = (any::<bool>(), any::<u32>(), 1..=100u32, any::<bool>());
    let care = (
        any::<bool>(),
        any::<u32>(),
        any::<bool>(),
        1..=u32::MAX,
        text(),
    );
    let rest = (
        care,
        (any::<bool>(), 1..=u32::MAX),
        one_of(&[
            LogLevel::Error,
            LogLevel::Warn,
            LogLevel::Info,
            LogLevel::Debug,
            LogLevel::Trace,
        ]),
    );
    (input, stale(), safety, prompt(), action(), rest).prop_map(
        |((idle, when_locked, locked, activity, backend), stale, safety, prompt, action, rest)| {
            let (ceiling_enabled, minutes, ceiling_stale_percent, ceiling_during_pause) = safety;
            let ((enabled, standby, reminder_enabled, reminder_hours, trigger_cmd), history, level) =
                rest;
            let shortest = u32::try_from(stale.normal_path_seconds() / 60 + 1).unwrap();
            Config {
                version: CURRENT_VERSION,
                idle: IdleConfig {
                    input_idle_minutes: idle,
                },
                session: SessionConfig {
                    when_locked,
                    locked_blank_seconds: locked,
                },
                activity,
                capture: CaptureConfig { backend },
                safety: SafetyConfig {
                    ceiling_enabled,
                    ceiling_minutes: minutes.max(shortest),
                    ceiling_stale_percent,
                    ceiling_during_pause,
                },
                stale,
                prompt,
                action,
                panel_care: PanelCareConfig {
                    enabled,
                    min_standby_minutes: standby,
                    reminder_enabled,
                    reminder_hours,
                    trigger_cmd,
                },
                history: HistoryConfig {
                    enabled: history.0,
                    max_entries: history.1,
                },
                logging: LoggingConfig { level },
            }
        },
    )
}

fn outside(allowed: &RangeInclusive<u32>) -> BoxedStrategy<u32> {
    let below = allowed.start().checked_sub(1).map(|top| (0..=top).boxed());
    let above = allowed
        .end()
        .checked_add(1)
        .map(|bottom| (bottom..=u32::MAX).boxed());
    Union::new(below.into_iter().chain(above)).boxed()
}

fn case(
    key: &'static str,
    values: impl Strategy<Value = u32> + 'static,
    set: Setter,
) -> BoxedStrategy<(&'static str, Setter, u32)> {
    values.prop_map(move |value| (key, set, value)).boxed()
}

fn cross_key_cases() -> Vec<BoxedStrategy<(&'static str, Setter, u32)>> {
    vec![
        case("stale.ignore_regions[0].w", Just(0), |c, v| {
            c.stale.ignore_regions.insert(0, ignore_region(v, 1));
        }),
        case("stale.ignore_regions[0].h", Just(0), |c, v| {
            c.stale.ignore_regions.insert(0, ignore_region(1, v));
        }),
        case("safety.ceiling_minutes", 0..=10u32, |c, v| {
            let normal_minutes = c.stale.normal_path_seconds() / 60;
            c.safety.ceiling_enabled = true;
            c.safety.ceiling_minutes = u32::try_from(normal_minutes).unwrap().saturating_sub(v);
        }),
        case("prompt.snooze_presets_minutes[0]", 1..=1000u32, |c, v| {
            let too_long = c.prompt.custom_max_minutes + v;
            c.prompt.snooze_presets_minutes.insert(0, too_long);
        }),
        case("prompt.custom_max_minutes", 1..=100u32, |c, v| {
            c.prompt.custom_max_minutes = c.prompt.custom_min_minutes.saturating_sub(v);
            c.prompt.custom_min_minutes = c.prompt.custom_min_minutes.max(v + 1);
        }),
    ]
}

fn out_of_range() -> impl Strategy<Value = (&'static str, Setter, u32)> {
    let ranges = RANGE_RULES
        .iter()
        .map(|rule| case(rule.key, outside(&rule.allowed), rule.set));
    Union::new(ranges.chain(cross_key_cases()))
}

fn ignore_region(w: u32, h: u32) -> IgnoreRegion {
    IgnoreRegion {
        output: "HDMI-A-1".to_owned(),
        x: 0,
        y: 0,
        w,
        h,
    }
}

proptest! {
    #[test]
    fn valid_configs_round_trip_through_toml(config in arb_config()) {
        prop_assert_eq!(config.validate(), Ok(()));
        let written = config.to_toml_string().unwrap();
        let outcome = Config::from_toml_str(&written).unwrap();
        prop_assert_eq!(outcome.config, config);
        prop_assert_eq!(outcome.migrated_from, None);
    }

    #[test]
    fn out_of_range_values_are_rejected_at_their_key(
        mut config in arb_config(),
        (key, set, value) in out_of_range(),
    ) {
        set(&mut config, value);
        let keys = issue_keys(&config);
        prop_assert!(keys.iter().any(|k| k == key), "{} = {} not in {:?}", key, value, keys);
    }
}
