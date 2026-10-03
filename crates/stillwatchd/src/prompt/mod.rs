//! Prompters: how Stillwatch asks before it acts.
//!
//! [`StylePrompter`] picks a notification or a dialog from `prompt.style`.
//! `auto` uses the notification until notifications are verified not to show
//! over a fullscreen surface. With `prompt.fallback_to_dialog`, a missing
//! server, a failed `Notify`, or a close without an action shows the dialog
//! for the countdown that is still left.

mod dialog;
mod fullscreen;
mod select;
mod style;

pub mod notification;

pub use dialog::{DialogLauncher, KdialogLauncher};
pub use fullscreen::{FullscreenMonitor, UnverifiedFullscreen};
pub use notification::NotificationPrompter;
pub use select::NOTIFICATIONS_HIDDEN_OVER_FULLSCREEN;
pub use style::{PromptParts, StylePrompter};
