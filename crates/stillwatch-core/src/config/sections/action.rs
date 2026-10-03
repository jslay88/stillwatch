//! `[action]`: what happens when the prompt times out.

use serde::{Deserialize, Serialize};

pub use crate::command::BlankMethod;
use crate::config::limits::PERCENT;
use crate::config::validate::Issues;

/// The action taken when the prompt times out.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionMode {
    /// Blank the displays.
    #[default]
    Blank,
    /// Lock the session, then blank.
    LockAndBlank,
    /// Dim for `dim_seconds`, then blank.
    DimThenBlank,
    /// Run `command` instead of blanking.
    Command,
}

/// Which outputs the action applies to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionOutputs {
    /// Only the monitored outputs.
    #[default]
    Monitored,
    /// Every connected output.
    All,
}

/// How the dim step dims.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DimMethod {
    /// A translucent black overlay.
    #[default]
    Overlay,
    /// Lower the screen brightness (KDE).
    Brightness,
}

/// What to do after `reblank_max_attempts` failed re-blanks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReblankFallback {
    /// Switch to the black overlay.
    #[default]
    Overlay,
    /// Give up re-blanking.
    None,
}

/// `[action]`: the action, its hooks, and the re-blank watchdog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ActionConfig {
    /// The action taken on timeout.
    pub mode: ActionMode,
    /// How displays are blanked.
    pub blank_method: BlankMethod,
    /// Which outputs are acted on.
    pub outputs: ActionOutputs,
    /// How the dim step dims.
    pub dim_method: DimMethod,
    /// Dim level as a percentage (0-100).
    pub dim_percent: u32,
    /// Seconds to stay dimmed before blanking.
    pub dim_seconds: u32,
    /// Command run when `mode = "command"`.
    pub command: String,
    /// Command run after blanking.
    pub on_blank_cmd: String,
    /// Command run on resume.
    pub on_resume_cmd: String,
    /// Whether to blank again when a display wakes without input.
    pub reblank_on_wake: bool,
    /// Seconds to wait before re-blanking.
    pub reblank_grace_seconds: u32,
    /// Re-blank attempts before falling back; 0 means unlimited.
    pub reblank_max_attempts: u32,
    /// What to do after the last failed re-blank.
    pub reblank_fallback: ReblankFallback,
}

impl Default for ActionConfig {
    fn default() -> Self {
        Self {
            mode: ActionMode::default(),
            blank_method: BlankMethod::default(),
            outputs: ActionOutputs::default(),
            dim_method: DimMethod::default(),
            dim_percent: 20,
            dim_seconds: 30,
            command: String::new(),
            on_blank_cmd: String::new(),
            on_resume_cmd: String::new(),
            reblank_on_wake: true,
            reblank_grace_seconds: 15,
            reblank_max_attempts: 3,
            reblank_fallback: ReblankFallback::default(),
        }
    }
}

impl ActionConfig {
    pub(crate) fn validate(&self, issues: &mut Issues) {
        issues.range("action.dim_percent", self.dim_percent, PERCENT);
        if self.mode == ActionMode::Command {
            issues.not_blank("action.command", &self.command);
        }
    }
}
