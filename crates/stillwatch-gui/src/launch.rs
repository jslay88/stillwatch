//! How a `stillwatch-gui` process was asked to start.

/// Which window, if any, to show when this process becomes the running GUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LaunchMode {
    /// Tray only, until the menu or a later invocation opens a window.
    Tray,
    /// Open the settings window.
    Settings,
    /// Open the prompt placeholder.
    Prompt,
}

impl LaunchMode {
    /// The wire value `Activate` carries.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tray => "tray",
            Self::Settings => "settings",
            Self::Prompt => "prompt",
        }
    }

    /// Parses an `Activate` argument. An empty string is the tray.
    ///
    /// # Errors
    ///
    /// Returns the bad value when it isn't a known mode.
    pub fn parse(mode: &str) -> Result<Self, String> {
        match mode {
            "" | "tray" => Ok(Self::Tray),
            "settings" => Ok(Self::Settings),
            "prompt" => Ok(Self::Prompt),
            other => Err(format!("unknown gui mode: {other}")),
        }
    }

    /// Which window to open when the session bus is missing.
    ///
    /// Tray mode has nothing to draw without a bus, so the settings window
    /// opens and the Quit button can exit.
    #[must_use]
    pub const fn without_bus(self) -> Self {
        match self {
            Self::Tray => Self::Settings,
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_values_round_trip() {
        for mode in [LaunchMode::Tray, LaunchMode::Settings, LaunchMode::Prompt] {
            assert_eq!(LaunchMode::parse(mode.as_str()), Ok(mode));
        }
        assert_eq!(LaunchMode::parse(""), Ok(LaunchMode::Tray));
        assert_eq!(
            LaunchMode::parse("calibrate"),
            Err("unknown gui mode: calibrate".to_owned())
        );
        assert_eq!(LaunchMode::Tray.without_bus(), LaunchMode::Settings);
        assert_eq!(LaunchMode::Settings.without_bus(), LaunchMode::Settings);
        assert_eq!(LaunchMode::Prompt.without_bus(), LaunchMode::Prompt);
    }
}
