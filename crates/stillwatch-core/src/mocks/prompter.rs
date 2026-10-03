use super::{CallLog, Script};
use crate::backend::{BackendError, BackendFuture, Prompter};
use crate::prompt::{PromptOutcome, PromptRequest, Reminder};

/// A [`Prompter`] that answers with queued outcomes and records every call.
///
/// With nothing queued, `show` never resolves, like a user who doesn't answer.
#[derive(Debug, Default)]
pub struct MockPrompter {
    outcomes: Script<PromptOutcome>,
    requests: CallLog<PromptRequest>,
    dismissals: CallLog<()>,
    reminders: CallLog<Reminder>,
}

impl MockPrompter {
    /// A prompter with nothing queued.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues the answer to the next `show`.
    pub fn push_outcome(&self, outcome: PromptOutcome) {
        self.outcomes.push(Ok(outcome));
    }

    /// Makes the next `show` fail.
    pub fn push_error(&self, error: BackendError) {
        self.outcomes.push(Err(error));
    }

    /// Every request passed to `show`, oldest first.
    #[must_use]
    pub fn requests(&self) -> Vec<PromptRequest> {
        self.requests.snapshot()
    }

    /// How many times `dismiss` was called.
    #[must_use]
    pub fn dismiss_count(&self) -> usize {
        self.dismissals.len()
    }

    /// Every reminder passed to `remind`, oldest first.
    #[must_use]
    pub fn reminders(&self) -> Vec<Reminder> {
        self.reminders.snapshot()
    }
}

impl Prompter for MockPrompter {
    fn show(&self, request: PromptRequest) -> BackendFuture<'_, PromptOutcome> {
        self.requests.push(request);
        match self.outcomes.pop() {
            Some(result) => Box::pin(std::future::ready(result)),
            None => Box::pin(std::future::pending()),
        }
    }

    fn dismiss(&self) -> BackendFuture<'_, ()> {
        self.dismissals.push(());
        Box::pin(std::future::ready(Ok(())))
    }

    fn remind(&self, reminder: Reminder) -> BackendFuture<'_, ()> {
        self.reminders.push(reminder);
        Box::pin(std::future::ready(Ok(())))
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::mocks::now_or_never;

    fn request() -> PromptRequest {
        PromptRequest {
            countdown: Duration::from_secs(60),
            presets: vec![Duration::from_mins(15), Duration::from_hours(1)],
            allow_custom: true,
        }
    }

    #[test]
    fn answers_with_queued_outcomes_then_waits() {
        let prompter = MockPrompter::new();
        let snooze = PromptOutcome::Snooze(Duration::from_mins(15));
        prompter.push_outcome(snooze);
        prompter.push_error(BackendError::Unavailable("no notification server".into()));

        assert_eq!(now_or_never(prompter.show(request())), Some(Ok(snooze)));
        assert!(matches!(
            now_or_never(prompter.show(request())),
            Some(Err(BackendError::Unavailable(_)))
        ));
        assert_eq!(now_or_never(prompter.show(request())), None);
        assert_eq!(prompter.requests(), vec![request(), request(), request()]);
    }

    #[test]
    fn records_dismissals_and_reminders() {
        let prompter = MockPrompter::new();
        let reminder = Reminder::PanelCare {
            screen_on: Duration::from_hours(4),
        };
        assert_eq!(now_or_never(prompter.dismiss()), Some(Ok(())));
        assert_eq!(now_or_never(prompter.remind(reminder)), Some(Ok(())));
        assert_eq!(prompter.dismiss_count(), 1);
        assert_eq!(prompter.reminders(), vec![reminder]);
    }
}
