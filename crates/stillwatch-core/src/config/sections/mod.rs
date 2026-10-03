//! One struct per config section, each with its documented defaults.

mod action;
mod activity;
mod detection;
mod housekeeping;
mod prompt;

pub use action::{
    ActionConfig, ActionMode, ActionOutputs, BlankMethod, DimMethod, ReblankFallback,
};
pub use activity::{ActivityConfig, IdleConfig, SessionConfig, WhenLocked, ignores_device_name};
pub use detection::{
    CaptureBackend, CaptureConfig, IgnoreRegion, SafetyConfig, StaleConfig, StaleRequire,
};
pub use housekeeping::{HistoryConfig, LogLevel, LoggingConfig, PanelCareConfig};
pub use prompt::{PromptConfig, PromptStyle, PromptUrgency};
