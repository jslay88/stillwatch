use stillwatch_core::config::ActivityConfig;

/// The `[activity]` gamepad keys the source needs.
///
/// The daemon builds this from the config and hands it to
/// [`EvdevGamepadSource::update_settings`](super::EvdevGamepadSource::update_settings)
/// on every reload. Open devices stay open; only filtering changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GamepadSettings {
    /// Absolute-axis movement below this percent of the axis range is
    /// ignored, so a drifting stick doesn't count as input. Values above 100
    /// are treated as 100.
    pub deadzone_percent: u8,
    /// Devices whose name contains any of these (case-insensitive) never emit
    /// activity. Blank entries are skipped.
    pub ignore_devices: Vec<String>,
}

impl From<&ActivityConfig> for GamepadSettings {
    fn from(config: &ActivityConfig) -> Self {
        Self {
            deadzone_percent: u8::try_from(config.gamepad_deadzone_percent.min(100)).unwrap_or(100),
            ignore_devices: config.gamepad_ignore_devices.clone(),
        }
    }
}

impl Default for GamepadSettings {
    fn default() -> Self {
        Self::from(&ActivityConfig::default())
    }
}

/// Case-insensitive name substrings from `gamepad_ignore_devices`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct IgnoreList {
    needles: Vec<String>,
}

impl IgnoreList {
    pub(crate) fn new<S: AsRef<str>>(patterns: &[S]) -> Self {
        let needles = patterns
            .iter()
            .map(|pattern| pattern.as_ref().trim().to_lowercase())
            .filter(|needle| !needle.is_empty())
            .collect();
        Self { needles }
    }

    pub(crate) fn matches(&self, name: &str) -> bool {
        if self.needles.is_empty() {
            return false;
        }
        let name = name.to_lowercase();
        self.needles.iter().any(|needle| name.contains(needle))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_config_defaults() {
        let settings = GamepadSettings::default();
        assert_eq!(settings.deadzone_percent, 15);
        assert_eq!(settings.ignore_devices, Vec::<String>::new());
    }

    #[test]
    fn converts_from_the_activity_config() {
        let config = ActivityConfig {
            gamepad_deadzone_percent: 40,
            gamepad_ignore_devices: vec!["pedals".into()],
            ..ActivityConfig::default()
        };
        assert_eq!(
            GamepadSettings::from(&config),
            GamepadSettings {
                deadzone_percent: 40,
                ignore_devices: vec!["pedals".into()],
            }
        );
    }

    #[test]
    fn out_of_range_deadzones_clamp_to_100() {
        let config = ActivityConfig {
            gamepad_deadzone_percent: 5000,
            ..ActivityConfig::default()
        };
        assert_eq!(GamepadSettings::from(&config).deadzone_percent, 100);
    }

    #[test]
    fn an_empty_list_ignores_nothing() {
        let list = IgnoreList::new::<&str>(&[]);
        assert!(!list.matches("Xbox Wireless Controller"));
        assert!(!list.matches(""));
    }

    #[test]
    fn matches_substrings_case_insensitively() {
        let list = IgnoreList::new(&["xbox", "FANATEC"]);
        assert!(list.matches("Xbox Wireless Controller"));
        assert!(list.matches("Microsoft X-Box 360 pad / XBOX"));
        assert!(list.matches("Fanatec ClubSport Pedals"));
        assert!(!list.matches("Sony DualSense"));
        assert!(!list.matches("X-Box 360 pad"));
    }

    #[test]
    fn matches_whole_names_and_unicode_case() {
        let list = IgnoreList::new(&["ÄRGER PAD"]);
        assert!(list.matches("ärger pad"));
        assert!(list.matches("Das Ärger Pad v2"));
    }

    #[test]
    fn blank_patterns_never_match_everything() {
        let list = IgnoreList::new(&["", "   "]);
        assert!(!list.matches("Anything"));
        assert_eq!(list, IgnoreList::default());
    }

    #[test]
    fn patterns_are_trimmed() {
        let list = IgnoreList::new(&["  dualsense "]);
        assert!(list.matches("Sony Interactive Entertainment DualSense Wireless Controller"));
    }
}
