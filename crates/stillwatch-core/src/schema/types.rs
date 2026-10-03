//! Building blocks of the settings schema: sections, settings, and controls.

use toml::{Table, Value};

use super::lookup;
use crate::config::Bounds;
use crate::config::limits::ANY;

/// The unit a duration setting is stored in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeUnit {
    /// Whole seconds.
    Seconds,
    /// Whole minutes.
    Minutes,
    /// Whole hours.
    Hours,
}

impl TimeUnit {
    /// Plural lowercase name, such as `minutes`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Seconds => "seconds",
            Self::Minutes => "minutes",
            Self::Hours => "hours",
        }
    }
}

/// One allowed value of an enum setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Choice {
    /// The value as written in the config file.
    pub value: &'static str,
    /// Short name for a select box.
    pub label: &'static str,
    /// What picking this value does.
    pub help: &'static str,
}

/// Which GUI control edits a setting, with its allowed values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    /// Shown but not edited; Stillwatch writes it itself.
    ReadOnly,
    /// On or off.
    Toggle,
    /// A whole number. `step` is the spin-button increment; values in between
    /// are still valid.
    Int {
        /// Allowed values.
        bounds: Bounds,
        /// Spin-button increment.
        step: u32,
    },
    /// A percentage slider.
    Percent {
        /// Allowed values.
        bounds: Bounds,
    },
    /// A duration stored as a whole number of `unit`.
    Duration {
        /// Unit the value is stored in.
        unit: TimeUnit,
        /// Allowed values, in `unit`.
        bounds: Bounds,
    },
    /// One value from a fixed list.
    Enum {
        /// Every allowed value, in display order.
        choices: &'static [Choice],
    },
    /// Free text.
    Text,
    /// A shell command line; empty means none.
    Command,
    /// An editable list of text entries.
    StringList,
    /// An editable list of whole numbers.
    IntList {
        /// Unit of each entry, when the entries are durations.
        unit: Option<TimeUnit>,
        /// Allowed values for each entry.
        bounds: Bounds,
    },
    /// A `[cols, rows]` pair.
    GridSize {
        /// Allowed values for each of cols and rows.
        bounds: Bounds,
    },
    /// Multi-select of connected outputs, fed by the daemon.
    OutputPicker,
    /// Multi-select of detected gamepads, fed by the daemon.
    GamepadPicker,
    /// Multi-select of MPRIS players, fed by the daemon.
    PlayerPicker,
    /// Rectangles drawn on the block-grid heatmap.
    RegionEditor,
}

impl Control {
    /// A short description of the value, such as `duration in minutes`.
    #[must_use]
    pub fn type_name(&self) -> String {
        let name = match self {
            Self::ReadOnly => "read-only integer",
            Self::Toggle => "boolean",
            Self::Int { .. } => "integer",
            Self::Percent { .. } => "percent",
            Self::Duration { unit, .. } => return format!("duration in {}", unit.name()),
            Self::Enum { .. } => "choice",
            Self::Text => "text",
            Self::Command => "command",
            Self::StringList => "list of text",
            Self::IntList {
                unit: Some(unit), ..
            } => return format!("list of durations in {}", unit.name()),
            Self::IntList { unit: None, .. } => "list of integers",
            Self::GridSize { .. } => "grid size (cols, rows)",
            Self::OutputPicker => "list of output names",
            Self::GamepadPicker => "list of gamepad name substrings",
            Self::PlayerPicker => "list of MPRIS player names",
            Self::RegionEditor => "list of regions",
        };
        name.to_owned()
    }

    /// The numeric range, for controls that have one.
    #[must_use]
    pub const fn bounds(&self) -> Option<Bounds> {
        match self {
            Self::Int { bounds, .. }
            | Self::Percent { bounds }
            | Self::Duration { bounds, .. }
            | Self::IntList { bounds, .. }
            | Self::GridSize { bounds } => Some(*bounds),
            _ => None,
        }
    }

    /// The allowed values, for enum controls.
    #[must_use]
    pub const fn choices(&self) -> Option<&'static [Choice]> {
        match self {
            Self::Enum { choices } => Some(choices),
            _ => None,
        }
    }

    /// The allowed values as text, such as `1 to 100` or `auto | kwin | portal`.
    ///
    /// `None` when any value of the type is allowed.
    #[must_use]
    pub fn allowed(&self) -> Option<String> {
        if let Some(choices) = self.choices() {
            let values: Vec<_> = choices.iter().map(|choice| choice.value).collect();
            return Some(values.join(" | "));
        }
        let bounds = self.bounds().filter(|bounds| *bounds != ANY)?;
        let each = matches!(self, Self::IntList { .. } | Self::GridSize { .. });
        Some(if each {
            format!("each {bounds}")
        } else {
            bounds.to_string()
        })
    }
}

/// One config key and how to edit it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Setting {
    /// Dotted key path as serialized, such as `stale.stale_percent`.
    pub key: &'static str,
    /// Short name for the settings window.
    pub label: &'static str,
    /// What the setting does and why its default is what it is.
    pub help: &'static str,
    /// How the GUI edits it, with its allowed values.
    pub control: Control,
    /// Changing it rebuilds the capture backend and resets the block counters.
    pub resets_detection: bool,
}

impl Setting {
    /// A setting that applies on reload without resetting detection.
    #[must_use]
    pub const fn new(
        key: &'static str,
        label: &'static str,
        control: Control,
        help: &'static str,
    ) -> Self {
        Self {
            key,
            label,
            help,
            control,
            resets_detection: false,
        }
    }

    /// Marks the setting as resetting detection when changed.
    #[must_use]
    pub const fn resetting_detection(self) -> Self {
        Self {
            resets_detection: true,
            ..self
        }
    }

    /// The key within its section, such as `stale_percent`.
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.key.rsplit_once('.').map_or(self.key, |(_, name)| name)
    }

    /// The default value, looked up in a serialized default config
    /// (see [`super::default_table`]).
    #[must_use]
    pub fn default_in<'a>(&self, defaults: &'a Table) -> Option<&'a Value> {
        lookup(defaults, self.key)
    }
}

/// A config section and its settings, in file order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Section {
    /// TOML table name, such as `stale`; empty for top-level keys.
    pub id: &'static str,
    /// Page title in the settings window.
    pub title: &'static str,
    /// What the section controls.
    pub help: &'static str,
    /// The section's settings.
    pub settings: &'static [Setting],
}
