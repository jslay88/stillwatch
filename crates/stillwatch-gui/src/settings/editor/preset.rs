//! Reviewing and applying a display preset. Private to the editor module.

use super::super::pickers::set_exact;
use super::super::presets::{self, PresetDraft};
use super::Editor;
use crate::edit_msg::PresetKind;

impl Editor {
    pub(super) fn preview_preset(&mut self, kind: PresetKind) {
        self.preset = Some(PresetDraft::new(kind));
    }

    pub(super) fn cancel_preset(&mut self) {
        self.preset = None;
    }

    pub(super) fn toggle_mixed(&mut self, name: &str, on: bool) {
        let Some(draft) = self.preset.as_mut() else {
            return;
        };
        if draft.kind != PresetKind::Mixed {
            return;
        }
        draft.outputs = set_exact(&draft.outputs, name, on);
    }

    pub(super) fn set_mixed_draft(&mut self, value: String) {
        let Some(draft) = self.preset.as_mut() else {
            return;
        };
        draft.draft = value;
    }

    pub(super) fn push_mixed(&mut self) {
        let Some(draft) = self.preset.as_mut() else {
            return;
        };
        if draft.kind != PresetKind::Mixed || draft.draft.is_empty() {
            return;
        }
        let name = std::mem::take(&mut draft.draft);
        if !draft.outputs.iter().any(|output| output == &name) {
            draft.outputs.push(name);
        }
    }

    /// Writes the open preset's keys into the form and closes the review.
    ///
    /// `false` when nothing was open or no key would change, so a custom
    /// preset and an already-matching one don't touch the file.
    pub(super) fn apply_preset_keys(&mut self) -> bool {
        let Some(draft) = self.preset.clone() else {
            return false;
        };
        let changed = presets::write(&mut self.values, &draft);
        self.preset = None;
        if changed {
            self.save_error = None;
        }
        changed
    }
}
