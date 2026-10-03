//! Which prompt to show for `prompt.style`, before any fallback.

use stillwatch_core::config::PromptStyle;
use stillwatch_core::history::{PromptMedium, PromptReason};

/// Whether a fullscreen surface is known to hide notifications.
///
/// Do Not Disturb and fullscreen were not checked on a live Plasma session
/// (that would poke the running desktop). `auto` keeps the notification
/// until this is set from a verified result.
pub const NOTIFICATIONS_HIDDEN_OVER_FULLSCREEN: bool = false;

const _: () = assert!(!NOTIFICATIONS_HIDDEN_OVER_FULLSCREEN);

/// Facts the style choice is allowed to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PromptFacts {
    /// A fullscreen surface is active.
    pub fullscreen: bool,
    /// Verified that notifications do not appear over that surface.
    pub notifications_hidden_over_fullscreen: bool,
}

/// The medium to show, and why, before fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Choice {
    /// Notification or dialog.
    pub medium: PromptMedium,
    /// Why that medium was picked.
    pub reason: PromptReason,
}

/// Picks a notification or a dialog.
///
/// `notification` and `dialog` ignore fullscreen. `auto` uses the dialog
/// only when a fullscreen surface is active **and** notifications are known
/// not to show over it. Until [`NOTIFICATIONS_HIDDEN_OVER_FULLSCREEN`] is
/// set, `auto` is the notification either way.
#[must_use]
pub const fn select(style: PromptStyle, facts: PromptFacts) -> Choice {
    match style {
        PromptStyle::Notification => Choice {
            medium: PromptMedium::Notification,
            reason: PromptReason::Configured,
        },
        PromptStyle::Dialog => Choice {
            medium: PromptMedium::Dialog,
            reason: PromptReason::Configured,
        },
        PromptStyle::Auto if facts.fullscreen && facts.notifications_hidden_over_fullscreen => {
            Choice {
                medium: PromptMedium::Dialog,
                reason: PromptReason::Fullscreen,
            }
        }
        PromptStyle::Auto => Choice {
            medium: PromptMedium::Notification,
            reason: PromptReason::Auto,
        },
    }
}

#[cfg(test)]
mod tests;
