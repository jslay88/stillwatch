//! Settings-window pages. Each one is a placeholder until its own issue.

/// A page of the settings window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    /// Schema-driven settings (later).
    #[default]
    Settings,
    /// Calibration heatmap (later).
    Calibration,
    /// Decision history (later).
    History,
    /// Daemon service controls (later).
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
            Self::Calibration => "The calibration view is not built yet.",
            Self::History => "History is not built yet.",
            Self::Service => "Service controls are not built yet.",
        }
    }
}
