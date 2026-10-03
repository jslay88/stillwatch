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
    /// Show the diff for a display preset. Nothing is written yet.
    PreviewPreset(PresetKind),
    /// Check or uncheck an output in the mixed OLED preset.
    MixedToggle {
        /// Connector name.
        name: String,
        /// Whether that output is OLED.
        on: bool,
    },
    /// Free-text output for the mixed preset, not yet in the selection.
    MixedDraft(String),
    /// Append the mixed-preset add row to the selection.
    MixedPush,
    /// Write the reviewed preset through the normal save.
    ApplyPreset,
    /// Drop the preset diff without writing.
    CancelPreset,
}

/// A starting point for a display setup. Custom changes nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresetKind {
    /// DPMS blank, re-blank on wake, overlay fallback.
    OledMonitor,
    /// Black overlay blank.
    OledTvOverlay,
    /// DPMS plus example TV power hooks.
    OledTvHooks,
    /// Watch the outputs the user marks as OLED.
    Mixed,
    /// Leave every key as it is.
    Custom,
}

impl PresetKind {
    /// Every preset, in the order the settings page shows them.
    pub const ALL: [Self; 5] = [
        Self::OledMonitor,
        Self::OledTvOverlay,
        Self::OledTvHooks,
        Self::Mixed,
        Self::Custom,
    ];

    /// Button label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::OledMonitor => "OLED monitor",
            Self::OledTvOverlay => "OLED TV, overlay",
            Self::OledTvHooks => "OLED TV, DPMS + hooks",
            Self::Mixed => "Mixed OLED + LCD",
            Self::Custom => "Custom",
        }
    }
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
    /// Replace a whole list. Pickers send this when a checkbox changes.
    ListSet {
        /// Schema key.
        key: String,
        /// The list after the click.
        items: Vec<String>,
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
    /// Replace one region, or append when `index` is `None`.
    ///
    /// The calibration page sends this after a drag so `x`, `y`, `w`, and `h`
    /// land together.
    RegionSet {
        /// Schema key.
        key: String,
        /// Region to replace. `None` appends.
        index: Option<usize>,
        /// Output connector name.
        output: String,
        /// Left edge, as text.
        x: String,
        /// Top edge, as text.
        y: String,
        /// Width, as text.
        w: String,
        /// Height, as text.
        h: String,
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
            | Self::ListSet { key, .. }
            | Self::Grid { key, .. }
            | Self::Region { key, .. }
            | Self::RegionPush { key }
            | Self::RegionRemove { key, .. }
            | Self::RegionSet { key, .. } => key,
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
