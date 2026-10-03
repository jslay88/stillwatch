use super::BackendFuture;
use crate::prompt::{PromptOutcome, PromptRequest, Reminder};

/// Shows the burn-in prompt and informational reminders.
pub trait Prompter: Send + Sync {
    /// Shows the prompt and resolves with how it ended.
    ///
    /// The prompter keeps its countdown display up to date itself. Showing a
    /// new prompt replaces any prompt still open. Dropping the future stops
    /// waiting for an answer but doesn't necessarily close the prompt; call
    /// [`dismiss`](Self::dismiss) for that. Returns `Err` if the prompt
    /// couldn't be shown (for example no notification server).
    fn show(&self, request: PromptRequest) -> BackendFuture<'_, PromptOutcome>;

    /// Closes the prompt if one is open. Closing nothing is not an error.
    fn dismiss(&self) -> BackendFuture<'_, ()>;

    /// Shows a dismissible reminder and returns once it has been sent.
    fn remind(&self, reminder: Reminder) -> BackendFuture<'_, ()>;
}
