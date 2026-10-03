//! Maps systemd `ActiveState` and unit-file state onto the service page.

/// `ActiveState` reduced to what the page says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunState {
    /// The unit is running.
    Active,
    /// The unit is stopped.
    Inactive,
    /// The unit failed.
    Failed,
    /// `activating`, `deactivating`, or anything else systemd reports.
    Other(String),
}

impl RunState {
    /// Maps an `ActiveState` string.
    #[must_use]
    pub fn from_active(active: &str) -> Self {
        match active {
            "active" => Self::Active,
            "inactive" => Self::Inactive,
            "failed" => Self::Failed,
            other => Self::Other(other.to_owned()),
        }
    }

    /// The failed state, which is when the page shows the journal.
    #[must_use]
    pub const fn is_failed(&self) -> bool {
        matches!(self, Self::Failed)
    }
}

impl std::fmt::Display for RunState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Active => formatter.write_str("active"),
            Self::Inactive => formatter.write_str("inactive"),
            Self::Failed => formatter.write_str("failed"),
            Self::Other(text) => formatter.write_str(text),
        }
    }
}

/// Whether `GetUnitFileState` means the unit starts at login.
///
/// `enabled` and the alias/link forms count. `disabled`, `static`, and
/// `masked` do not.
#[must_use]
pub fn enabled_at_login(file_state: &str) -> bool {
    matches!(
        file_state,
        "enabled"
            | "enabled-runtime"
            | "linked"
            | "linked-runtime"
            | "alias"
            | "indirect"
            | "generated"
    )
}

/// What the service page knows about `stillwatch.service`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum UnitView {
    /// Not queried yet.
    #[default]
    Unknown,
    /// The unit file is not installed.
    Missing,
    /// The user manager isn't there, or the call failed.
    Error(String),
    /// Installed. `journal` is filled only while failed.
    Ready {
        /// Running, stopped, or failed.
        run: RunState,
        /// Enabled for the graphical session.
        enabled: bool,
        /// Raw `GetUnitFileState` value.
        file_state: String,
        /// Recent journal lines while [`RunState::Failed`].
        journal: Vec<String>,
    },
}

impl UnitView {
    /// A present unit. `enabled` comes from `file_state`.
    #[must_use]
    pub fn ready(active: &str, file_state: &str, journal: Vec<String>) -> Self {
        Self::Ready {
            run: RunState::from_active(active),
            enabled: enabled_at_login(file_state),
            file_state: file_state.to_owned(),
            journal,
        }
    }

    /// Journal lines when the unit is failed.
    #[must_use]
    pub fn journal(&self) -> &[String] {
        match self {
            Self::Ready { run, journal, .. } if run.is_failed() => journal,
            Self::Ready { .. } | Self::Unknown | Self::Missing | Self::Error(_) => &[],
        }
    }
}

/// The sentence under the Service heading.
#[must_use]
pub fn unit_sentence(unit: &UnitView) -> String {
    match unit {
        UnitView::Unknown => "Checking stillwatch.service.".to_owned(),
        UnitView::Missing => "stillwatch.service is not installed.".to_owned(),
        UnitView::Error(text) => text.clone(),
        UnitView::Ready {
            run, file_state, ..
        } => format!("stillwatch.service is {run} ({file_state})."),
    }
}
