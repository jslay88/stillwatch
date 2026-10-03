//! Settings form: values, dirty state, validation, and the save.

use std::collections::BTreeMap;
use std::path::Path;

use stillwatch_core::config::{Config, ConfigError, LoadOutcome};
use stillwatch_core::schema;

use crate::edit_msg::RestoreScope;
use stillwatch_ipc::config_file;

use super::assemble;
use super::presets::PresetDraft;
use super::values::{FieldError, FieldValue, RegionInput};

mod edit;
mod preset;

pub use edit::handle;

/// What [`handle`] did, for the shell to apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// Ask the daemon to `Reload()` when it is running.
    pub reload: bool,
    /// New snooze presets, when the form was saved or reloaded.
    pub presets: Option<Vec<u32>>,
}

impl Outcome {
    fn none() -> Self {
        Self {
            reload: false,
            presets: None,
        }
    }

    fn saved(presets: Vec<u32>) -> Self {
        Self {
            reload: true,
            presets: Some(presets),
        }
    }
}

/// The file changed while the form had unsaved edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Banner {
    /// Offer reload-from-disk or keep-my-edits.
    DiskChanged,
}

/// The settings form. No widgets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Editor {
    values: BTreeMap<String, FieldValue>,
    baseline: BTreeMap<String, FieldValue>,
    drafts: BTreeMap<String, String>,
    document: String,
    migrated: bool,
    banner: Option<Banner>,
    pending: Option<RestoreScope>,
    save_error: Option<String>,
    load_error: Option<String>,
    preset: Option<PresetDraft>,
}

impl Editor {
    /// Defaults, with nothing to write until the user edits.
    #[must_use]
    pub fn pristine() -> Self {
        Self::from_config(&Config::default(), "").unwrap_or_else(Self::broken)
    }

    /// Loads `path`. A missing file is the defaults.
    ///
    /// # Errors
    ///
    /// Returns a message when the file can't be read, parsed, or validated.
    pub fn load(path: &Path) -> Result<Self, String> {
        match config_file::read(path) {
            Ok(text) => Self::from_text(&text),
            Err(ConfigError::NotFound { .. }) => Ok(Self::pristine()),
            Err(err) => Err(err.to_string()),
        }
    }

    /// Builds a form from text that already parsed.
    ///
    /// # Errors
    ///
    /// Returns a message when a schema key has no value in `outcome`.
    pub fn from_loaded(text: &str, outcome: &LoadOutcome) -> Result<Self, String> {
        let values = assemble::fields_of(&outcome.config)?;
        Ok(Self {
            baseline: values.clone(),
            values,
            drafts: BTreeMap::new(),
            document: text.to_owned(),
            migrated: outcome.migrated_from.is_some(),
            banner: None,
            pending: None,
            save_error: None,
            load_error: None,
            preset: None,
        })
    }

    /// The value for `key`, when the form has one.
    #[must_use]
    pub fn field(&self, key: &str) -> Option<&FieldValue> {
        self.values.get(key)
    }

    /// A whole-number field, when it parses.
    #[must_use]
    pub fn number(&self, key: &str) -> Option<u32> {
        match self.values.get(key)? {
            FieldValue::Text(text) => text.trim().parse().ok(),
            _ => None,
        }
    }

    /// Ignore regions on the form, including ones not saved yet.
    #[must_use]
    pub fn ignore_regions(&self) -> &[RegionInput] {
        match self.values.get("stale.ignore_regions") {
            Some(FieldValue::Regions(regions)) => regions,
            _ => &[],
        }
    }

    /// Text in the add row of a list.
    #[must_use]
    pub fn draft(&self, key: &str) -> &str {
        self.drafts.get(key).map_or("", String::as_str)
    }

    /// The disk-changed banner, when one is showing.
    #[must_use]
    pub fn banner(&self) -> Option<Banner> {
        self.banner
    }

    /// A restore waiting for confirmation.
    #[must_use]
    pub fn pending(&self) -> Option<&RestoreScope> {
        self.pending.as_ref()
    }

    /// The last save failure.
    #[must_use]
    pub fn save_error(&self) -> Option<&str> {
        self.save_error.as_deref()
    }

    /// The last failure reading the file.
    #[must_use]
    pub fn load_error(&self) -> Option<&str> {
        self.load_error.as_deref()
    }

    /// Records a failed read without throwing away the form.
    pub fn set_load_error(&mut self, message: String) {
        self.load_error = Some(message);
    }

    /// Unsaved edits, a pending list row, or a migrated file not written back yet.
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.migrated || self.values != self.baseline || self.has_draft()
    }

    /// Save is allowed while the form is dirty and the core validator is happy.
    #[must_use]
    pub fn can_save(&self) -> bool {
        self.is_dirty() && self.issues().is_empty()
    }

    /// Parse problems and [`Config`](Config) validation issues for the current form.
    #[must_use]
    pub fn issues(&self) -> Vec<FieldError> {
        assemble::issues(&self.effective())
    }

    /// The display preset being reviewed, when one is open.
    #[must_use]
    pub fn preset(&self) -> Option<&PresetDraft> {
        self.preset.as_ref()
    }

    /// Keys the open preset would change. Empty when nothing is open.
    #[must_use]
    pub fn preset_changes(&self) -> Vec<super::presets::KeyChange> {
        self.preset
            .as_ref()
            .map(|draft| super::presets::changes(&self.values, draft))
            .unwrap_or_default()
    }

    /// Snooze presets when the form is valid.
    #[must_use]
    pub fn presets(&self) -> Option<Vec<u32>> {
        assemble::config(&self.effective())
            .ok()
            .map(|config| config.prompt.snooze_presets_minutes)
    }

    /// Question shown before a restore, when one is pending.
    #[must_use]
    pub fn confirm_prompt(&self) -> Option<String> {
        match &self.pending {
            None => None,
            Some(RestoreScope::All) => Some(
                "Restore every setting to its default? Nothing is written until you save."
                    .to_owned(),
            ),
            Some(RestoreScope::Section(id)) => {
                let title = schema::SECTIONS
                    .iter()
                    .find(|section| section.id == id)
                    .map_or(id.as_str(), |section| section.title);
                Some(format!(
                    "Restore {title} to its defaults? Nothing is written until you save."
                ))
            }
        }
    }

    /// Refreshes from `path` when the form is clean, or raises the banner.
    pub fn on_disk_changed(&mut self, path: &Path) {
        if self.is_dirty() {
            self.banner = Some(Banner::DiskChanged);
            return;
        }
        self.reload(path);
    }

    fn from_text(text: &str) -> Result<Self, String> {
        let outcome = Config::from_toml_str(text).map_err(|err| err.to_string())?;
        Self::from_loaded(text, &outcome)
    }

    fn from_config(config: &Config, document: &str) -> Result<Self, String> {
        let values = assemble::fields_of(config)?;
        Ok(Self {
            baseline: values.clone(),
            values,
            drafts: BTreeMap::new(),
            document: document.to_owned(),
            migrated: false,
            banner: None,
            pending: None,
            save_error: None,
            load_error: None,
            preset: None,
        })
    }

    fn broken(message: String) -> Self {
        Self {
            values: BTreeMap::new(),
            baseline: BTreeMap::new(),
            drafts: BTreeMap::new(),
            document: String::new(),
            migrated: false,
            banner: None,
            pending: None,
            save_error: None,
            load_error: Some(message),
            preset: None,
        }
    }

    fn reload(&mut self, path: &Path) {
        match Self::load(path) {
            Ok(fresh) => *self = fresh,
            Err(err) => self.load_error = Some(err),
        }
    }

    fn effective(&self) -> BTreeMap<String, FieldValue> {
        let mut values = self.values.clone();
        for (key, draft) in &self.drafts {
            if draft.is_empty() {
                continue;
            }
            if let Some(FieldValue::List(items)) = values.get_mut(key) {
                items.push(draft.clone());
            }
        }
        values
    }

    fn has_draft(&self) -> bool {
        self.drafts.values().any(|draft| !draft.is_empty())
    }

    fn commit_drafts(&mut self) {
        let drafts = std::mem::take(&mut self.drafts);
        for (key, draft) in drafts {
            if draft.is_empty() {
                continue;
            }
            if let Some(FieldValue::List(items)) = self.values.get_mut(&key) {
                items.push(draft);
            }
        }
    }
}
