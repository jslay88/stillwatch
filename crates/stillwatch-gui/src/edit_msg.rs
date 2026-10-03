//! Inputs from the settings page. [`crate::model::update`] applies them.

/// What the settings page asked the model to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsMsg {
    /// A control changed.
    Edit(FieldChange),
    /// Write the file, then reload the daemon when it is running.
    Save,
    /// Replace the form with the file on disk.
    ReloadDisk,
    /// Dismiss the disk-changed banner and keep the form.
    KeepEdits,
    /// Ask before restoring defaults.
    AskRestore(RestoreScope),
    /// The user confirmed the pending restore.
    ConfirmRestore,
    /// The user dismissed the confirmation.
    CancelRestore,
}

/// One control edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldChange {
    /// A toggle flipped.
    Bool {
        /// Schema key.
        key: String,
        /// New value.
        value: bool,
    },
    /// Text, a number, or an enum value.
    Text {
        /// Schema key.
        key: String,
        /// New text.
        value: String,
    },
    /// One entry in a list.
    ListItem {
        /// Schema key.
        key: String,
        /// Index in the list.
        index: usize,
        /// New text.
        value: String,
    },
    /// The "add" row of a list, not yet part of the list.
    ListDraft {
        /// Schema key.
        key: String,
        /// Text in the add row.
        value: String,
    },
    /// Append the add row to the list.
    ListPush {
        /// Schema key.
        key: String,
    },
    /// Remove one list entry.
    ListRemove {
        /// Schema key.
        key: String,
        /// Index to remove.
        index: usize,
    },
    /// A `[cols, rows]` pair.
    Grid {
        /// Schema key.
        key: String,
        /// Column count, as text.
        cols: String,
        /// Row count, as text.
        rows: String,
    },
    /// One field of one ignore-region.
    Region {
        /// Schema key.
        key: String,
        /// Index in the region list.
        index: usize,
        /// Which part of the region.
        field: RegionPart,
        /// New text.
        value: String,
    },
    /// Append a blank region.
    RegionPush {
        /// Schema key.
        key: String,
    },
    /// Remove one region.
    RegionRemove {
        /// Schema key.
        key: String,
        /// Index to remove.
        index: usize,
    },
}

impl FieldChange {
    /// The schema key this edit writes.
    #[must_use]
    pub fn key(&self) -> &str {
        match self {
            Self::Bool { key, .. }
            | Self::Text { key, .. }
            | Self::ListItem { key, .. }
            | Self::ListDraft { key, .. }
            | Self::ListPush { key }
            | Self::ListRemove { key, .. }
            | Self::Grid { key, .. }
            | Self::Region { key, .. }
            | Self::RegionPush { key }
            | Self::RegionRemove { key, .. } => key,
        }
    }
}

/// One corner of an ignore-region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionPart {
    /// Output connector name.
    Output,
    /// Left edge.
    X,
    /// Top edge.
    Y,
    /// Width.
    W,
    /// Height.
    H,
}

/// Whose defaults a restore replaces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreScope {
    /// One section, by its schema id (`""` for top-level keys).
    Section(String),
    /// Every setting.
    All,
}
