//! Settings-window pages.

/// A page of the settings window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    /// Schema-driven settings.
    #[default]
    Settings,
    /// Calibration heatmap and ignore-region editor.
    Calibration,
    /// Decision history.
    History,
    /// Daemon service controls, tray autostart, and panel care.
    Service,
}

impl Page {
    /// Every page, in navigation order.
    pub const ALL: [Self; 4] = [
        Self::Settings,
        Self::Calibration,
        Self::History,
        Self::Service,
    ];

    /// The label shown on the navigation button and the page heading.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Settings => "Settings",
            Self::Calibration => "Calibration",
            Self::History => "History",
            Self::Service => "Service",
        }
    }

    /// The placeholder sentence for this page.
    #[must_use]
    pub const fn placeholder(self) -> &'static str {
        match self {
            Self::Settings => "Settings editing is not built yet.",
            Self::Calibration => "The calibration heatmap is on this page.",
            Self::History => "Past decisions, from the daemon or the history file.",
            Self::Service => "User unit, tray autostart, and panel care.",
        }
    }
}
