//! The interim dialog. The iced window is a later issue; until then
//! [`KdialogLauncher`] runs `kdialog` and maps the menu choice onto a
//! [`PromptOutcome`](stillwatch_core::prompt::PromptOutcome).
//!
//! The GUI dialog answers through the existing D-Bus `PromptAnswer` method,
//! which becomes that same outcome. No new bus method.

mod kdialog;
mod menu;

pub use kdialog::KdialogLauncher;

use stillwatch_core::backend::BackendFuture;
use stillwatch_core::prompt::{PromptOutcome, PromptRequest};

/// Shows the dialog and waits for an answer.
///
/// Dropping [`launch`](Self::launch) should close the dialog.
/// [`dismiss`](Self::dismiss) does that from another task.
pub trait DialogLauncher: Send + Sync {
    /// Shows `request` and resolves with how it ended.
    fn launch(&self, request: PromptRequest) -> BackendFuture<'_, PromptOutcome>;

    /// Closes the dialog if one is open. Closing nothing is not an error.
    fn dismiss(&self) -> BackendFuture<'_, ()>;
}
