//! Applying edits, restores, and saves. Private to the editor module.

use std::path::Path;

use stillwatch_core::config::{Config, ConfigError};
use stillwatch_core::schema::{self, Control};
use stillwatch_ipc::config_file;

use crate::edit_msg::{FieldChange, RestoreScope, SettingsMsg};

use super::super::values::{FieldValue, RegionInput};
use super::super::{assemble, document};
use super::{Editor, Outcome};

impl Editor {
    pub(super) fn apply_edit(&mut self, change: FieldChange) {
        if schema::find(change.key())
            .is_some_and(|setting| matches!(setting.control, Control::ReadOnly))
        {
            return;
        }
        self.save_error = None;
        match change {
            FieldChange::ListDraft { key, value } => {
                self.drafts.insert(key, value);
            }
            FieldChange::ListPush { key } => self.push_draft(&key),
            other => self.write_field(other),
        }
    }

    fn push_draft(&mut self, key: &str) {
        let Some(draft) = self.drafts.remove(key) else {
            return;
        };
        if draft.is_empty() {
            return;
        }
        if let Some(FieldValue::List(items)) = self.values.get_mut(key) {
            items.push(draft);
        }
    }

    fn write_field(&mut self, change: FieldChange) {
        let key = change.key().to_owned();
        let Some(field) = self.values.get_mut(&key) else {
            return;
        };
        match change {
            FieldChange::Bool { value, .. } => *field = FieldValue::Bool(value),
            FieldChange::Text { value, .. } => *field = FieldValue::Text(value),
            FieldChange::ListItem { index, value, .. } => {
                if let FieldValue::List(items) = field
                    && let Some(slot) = items.get_mut(index)
                {
                    *slot = value;
                }
            }
            FieldChange::ListRemove { index, .. } => {
                if let FieldValue::List(items) = field
                    && index < items.len()
                {
                    items.remove(index);
                }
            }
            FieldChange::ListSet { items, .. } => {
                if let FieldValue::List(slot) = field {
                    *slot = items;
                }
            }
            FieldChange::Grid { cols, rows, .. } => *field = FieldValue::Grid { cols, rows },
            FieldChange::Region {
                index,
                field: part,
                value,
                ..
            } => {
                if let FieldValue::Regions(regions) = field
                    && let Some(region) = regions.get_mut(index)
                {
                    region.set(part, value);
                }
            }
            FieldChange::RegionPush { .. } => {
                if let FieldValue::Regions(regions) = field {
                    regions.push(RegionInput::blank());
                }
            }
            FieldChange::RegionRemove { index, .. } => {
                if let FieldValue::Regions(regions) = field
                    && index < regions.len()
                {
                    regions.remove(index);
                }
            }
            FieldChange::RegionSet {
                index,
                output,
                x,
                y,
                w,
                h,
                ..
            } => {
                if let FieldValue::Regions(regions) = field {
                    let region = RegionInput { output, x, y, w, h };
                    match index {
                        Some(index) if index < regions.len() => regions[index] = region,
                        Some(_) | None => regions.push(region),
                    }
                }
            }
            FieldChange::ListDraft { .. } | FieldChange::ListPush { .. } => {}
        }
    }

    pub(super) fn ask_restore(&mut self, scope: RestoreScope) {
        self.pending = Some(scope);
    }

    pub(super) fn confirm_restore(&mut self) {
        let Some(scope) = self.pending.take() else {
            return;
        };
        let Ok(defaults) = assemble::fields_of(&Config::default()) else {
            return;
        };
        match scope {
            RestoreScope::All => self.values = defaults,
            RestoreScope::Section(id) => {
                for setting in schema::settings().filter(|setting| section_of(setting.key) == id) {
                    if let Some(value) = defaults.get(setting.key) {
                        self.values.insert(setting.key.to_owned(), value.clone());
                    }
                }
            }
        }
        self.save_error = None;
    }

    pub(super) fn cancel_restore(&mut self) {
        self.pending = None;
    }

    /// `None` when there was nothing to write.
    pub(super) fn save(&mut self, path: Option<&Path>) -> Result<Option<Vec<u32>>, String> {
        if !self.is_dirty() {
            return Ok(None);
        }
        let Some(path) = path else {
            return Err("can't find the config directory".to_owned());
        };
        self.commit_drafts();
        if !self.issues().is_empty() {
            return Err("fix the errors before saving".to_owned());
        }
        let config = assemble::config(&self.values).map_err(|errors| {
            errors
                .into_iter()
                .next()
                .map_or_else(|| "the config is invalid".to_owned(), |issue| issue.message)
        })?;
        let base = read_base(path, &self.document)?;
        let rendered = document::apply(&base, &config)?;
        config_file::write(path, &rendered, true).map_err(|err| err.to_string())?;
        self.document = rendered;
        self.baseline = self.values.clone();
        self.migrated = false;
        self.banner = None;
        self.save_error = None;
        self.load_error = None;
        Ok(Some(config.prompt.snooze_presets_minutes))
    }
}

/// Applies `message` to `editor`. `path` is the config file.
#[must_use]
pub fn handle(editor: &mut Editor, path: Option<&Path>, message: SettingsMsg) -> Outcome {
    match message {
        SettingsMsg::Edit(change) => {
            editor.apply_edit(change);
            Outcome::none()
        }
        SettingsMsg::Save => finish_save(editor, path),
        SettingsMsg::ReloadDisk => {
            if let Some(path) = path {
                editor.reload(path);
            }
            Outcome {
                reload: false,
                presets: editor.presets(),
            }
        }
        SettingsMsg::KeepEdits => {
            editor.banner = None;
            Outcome::none()
        }
        SettingsMsg::AskRestore(scope) => {
            editor.ask_restore(scope);
            Outcome::none()
        }
        SettingsMsg::ConfirmRestore => {
            editor.confirm_restore();
            Outcome::none()
        }
        SettingsMsg::CancelRestore => {
            editor.cancel_restore();
            Outcome::none()
        }
        SettingsMsg::PreviewPreset(kind) => {
            editor.preview_preset(kind);
            Outcome::none()
        }
        SettingsMsg::MixedToggle { name, on } => {
            editor.toggle_mixed(&name, on);
            Outcome::none()
        }
        SettingsMsg::MixedDraft(value) => {
            editor.set_mixed_draft(value);
            Outcome::none()
        }
        SettingsMsg::MixedPush => {
            editor.push_mixed();
            Outcome::none()
        }
        SettingsMsg::CancelPreset => {
            editor.cancel_preset();
            Outcome::none()
        }
        SettingsMsg::ApplyPreset => {
            if !editor.apply_preset_keys() {
                return Outcome::none();
            }
            finish_save(editor, path)
        }
    }
}

fn finish_save(editor: &mut Editor, path: Option<&Path>) -> Outcome {
    match editor.save(path) {
        Ok(Some(presets)) => Outcome::saved(presets),
        Ok(None) => Outcome::none(),
        Err(err) => {
            editor.save_error = Some(err);
            Outcome::none()
        }
    }
}

fn section_of(key: &str) -> &str {
    key.split_once('.').map_or("", |(section, _)| section)
}

fn read_base(path: &Path, loaded: &str) -> Result<String, String> {
    match config_file::read(path) {
        Ok(text) => Ok(text),
        Err(ConfigError::NotFound { .. }) if !loaded.is_empty() => Ok(loaded.to_owned()),
        Err(ConfigError::NotFound { .. }) => {
            schema::commented_toml().map_err(|err| err.to_string())
        }
        Err(err) => Err(err.to_string()),
    }
}
