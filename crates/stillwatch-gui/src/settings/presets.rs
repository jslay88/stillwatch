//! Display presets. Each one writes real config keys and nothing else.
//!
//! There is no `profile` key. Custom leaves the form alone. Applying a preset
//! goes through the same save as the Save button.

use std::collections::BTreeMap;

use super::values::{self, FieldValue};
use crate::edit_msg::PresetKind;

/// Example `on_blank_cmd` written by the OLED TV hook preset.
///
/// Same command the README shows for a TV that blanks over the network.
/// The diff shows it before anything is saved, so it can be cancelled.
pub const OLED_TV_BLANK_HOOK: &str = "lg-webos-cli screen-off";

/// Example `on_resume_cmd` paired with [`OLED_TV_BLANK_HOOK`].
pub const OLED_TV_RESUME_HOOK: &str = "lg-webos-cli screen-on";

/// Shown while reviewing the mixed OLED + LCD preset.
pub const MIXED_HELP: &str =
    "KWin DPMS cannot spare the LCD. This preset blanks the monitored outputs with the overlay.";

/// One key a preset would change, as the form currently shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyChange {
    /// Dotted schema key.
    pub key: &'static str,
    /// Value before the preset.
    pub before: String,
    /// Value the preset writes.
    pub after: String,
}

/// The preset the user is reviewing, including the mixed-output selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetDraft {
    /// Which preset is open.
    pub kind: PresetKind,
    /// Outputs chosen as OLED for [`PresetKind::Mixed`].
    pub outputs: Vec<String>,
    /// Free-text output not yet added to [`Self::outputs`].
    pub draft: String,
}

impl PresetDraft {
    /// A preset with no mixed-output selection yet.
    #[must_use]
    pub fn new(kind: PresetKind) -> Self {
        Self {
            kind,
            outputs: Vec::new(),
            draft: String::new(),
        }
    }
}

/// Keys `draft` would change on `values`, in the preset's documented order.
#[must_use]
pub fn changes(values: &BTreeMap<String, FieldValue>, draft: &PresetDraft) -> Vec<KeyChange> {
    targets(draft.kind, &draft.outputs)
        .into_iter()
        .filter_map(|(key, next)| {
            let current = values.get(key)?;
            if current == &next {
                return None;
            }
            Some(KeyChange {
                key,
                before: values::display(current),
                after: values::display(&next),
            })
        })
        .collect()
}

/// Writes `draft` onto `values`. Returns whether any key changed.
pub fn write(values: &mut BTreeMap<String, FieldValue>, draft: &PresetDraft) -> bool {
    let mut changed = false;
    for (key, next) in targets(draft.kind, &draft.outputs) {
        let Some(slot) = values.get_mut(key) else {
            continue;
        };
        if *slot != next {
            *slot = next;
            changed = true;
        }
    }
    changed
}

/// The keys a preset owns, whether or not the form already has them.
#[cfg(test)]
#[must_use]
pub fn owned_keys(kind: PresetKind) -> &'static [&'static str] {
    match kind {
        PresetKind::OledMonitor => &[
            "action.blank_method",
            "action.reblank_on_wake",
            "action.reblank_fallback",
        ],
        PresetKind::OledTvOverlay => &["action.blank_method"],
        PresetKind::OledTvHooks => &[
            "action.blank_method",
            "action.on_blank_cmd",
            "action.on_resume_cmd",
        ],
        PresetKind::Mixed => &[
            "stale.monitored_outputs",
            "action.outputs",
            "action.blank_method",
        ],
        PresetKind::Custom => &[],
    }
}

fn targets(kind: PresetKind, mixed: &[String]) -> Vec<(&'static str, FieldValue)> {
    match kind {
        PresetKind::OledMonitor => vec![
            ("action.blank_method", text("dpms")),
            ("action.reblank_on_wake", FieldValue::Bool(true)),
            ("action.reblank_fallback", text("overlay")),
        ],
        PresetKind::OledTvOverlay => vec![("action.blank_method", text("overlay"))],
        PresetKind::OledTvHooks => vec![
            ("action.blank_method", text("dpms")),
            ("action.on_blank_cmd", text(OLED_TV_BLANK_HOOK)),
            ("action.on_resume_cmd", text(OLED_TV_RESUME_HOOK)),
        ],
        PresetKind::Mixed => vec![
            ("stale.monitored_outputs", FieldValue::List(mixed.to_vec())),
            ("action.outputs", text("monitored")),
            ("action.blank_method", text("overlay")),
        ],
        PresetKind::Custom => Vec::new(),
    }
}

fn text(value: &str) -> FieldValue {
    FieldValue::Text(value.to_owned())
}

#[cfg(test)]
mod tests;
