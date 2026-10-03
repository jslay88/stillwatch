//! The prompt dialog.
//!
//! [`GuiLauncher`] runs `stillwatch-gui prompt`. The window calls the
//! existing D-Bus `PromptAnswer` method and exits. This launcher does not
//! report that click again. A process that exits 1, the same code the old
//! `kdialog` menu used for a close, is
//! [`PromptOutcome::Dismissed`](stillwatch_core::prompt::PromptOutcome::Dismissed).
//!
//! Dropping [`launch`](DialogLauncher::launch) should stop waiting.
//! [`dismiss`](DialogLauncher::dismiss) kills the GUI process.

mod gui;

pub use gui::GuiLauncher;

use stillwatch_core::backend::BackendFuture;
use stillwatch_core::prompt::{PromptOutcome, PromptRequest};

/// Shows the dialog and waits for an answer.
///
/// Dropping [`launch`](Self::launch) should close the dialog.
/// [`dismiss`](Self::dismiss) does that from another task.
pub trait DialogLauncher: Send + Sync {
    /// Shows `request` and resolves with how it ended.
    ///
    /// A GUI that already delivered the click over D-Bus must not resolve
    /// with that same outcome.
    fn launch(&self, request: PromptRequest) -> BackendFuture<'_, PromptOutcome>;

    /// Shows `request` on the custom duration field.
    ///
    /// The default is [`launch`](Self::launch). The notification's
    /// "Custom..." action uses this and drops the result: the GUI's later
    /// `PromptAnswer` is the real answer, and returning one here would be a
    /// second state transition.
    fn launch_custom(&self, request: PromptRequest) -> BackendFuture<'_, PromptOutcome> {
        self.launch(request)
    }

    /// Closes the dialog if one is open. Closing nothing is not an error.
    fn dismiss(&self) -> BackendFuture<'_, ()>;
}
