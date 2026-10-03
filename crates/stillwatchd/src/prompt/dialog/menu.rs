//! `kdialog --menu` arguments, and what its exit means.

use std::time::Duration;

use stillwatch_core::backend::BackendError;
use stillwatch_core::prompt::{PromptOutcome, PromptRequest};

use crate::process::{CommandError, CommandOutput, CommandResult, CommandSpec};
use crate::prompt::notification::{self, APP_ID};

/// How long `kdialog` may sit after the countdown. The state machine owns
/// the real timeout and dismisses the dialog; this only bounds a stuck process.
const GRACE: Duration = Duration::from_secs(30);

/// `kdialog` exits 1 when the dialog is cancelled or closed.
const CANCELLED: i32 = 1;

/// The `kdialog --menu` invocation for `request`.
pub(super) fn menu_spec(request: &PromptRequest) -> CommandSpec {
    let timeout = request.countdown.saturating_add(GRACE);
    let text = format!(
        "{}\n{}",
        notification::prompt_summary(request.countdown),
        notification::prompt_body(&request.stale_outputs)
    );
    let mut spec = CommandSpec::new("kdialog", timeout)
        .arg("--title")
        .arg(APP_ID)
        .arg("--menu")
        .arg(text);
    for pair in notification::prompt_actions(request).chunks(2) {
        if let [key, label] = pair {
            spec = spec.arg(key.clone()).arg(label.clone());
        }
    }
    spec
}

/// Maps a finished `kdialog` onto an outcome.
///
/// Exit 1 (cancel or close) is [`PromptOutcome::Dismissed`]. A selected tag
/// uses the same keys as the notification actions.
pub(super) fn outcome_of(result: CommandResult) -> Result<PromptOutcome, BackendError> {
    match result {
        Ok(output) => outcome_from_stdout(&output),
        Err(CommandError::Failed {
            code: Some(CANCELLED),
            ..
        }) => Ok(PromptOutcome::Dismissed),
        Err(error) => Err(error.into()),
    }
}

fn outcome_from_stdout(output: &CommandOutput) -> Result<PromptOutcome, BackendError> {
    let key = output.stdout.trim();
    notification::action_outcome(key).ok_or_else(|| {
        BackendError::Protocol(format!("kdialog returned unexpected choice {key:?}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use stillwatch_core::prompt::StaleOutput;

    fn request() -> PromptRequest {
        PromptRequest {
            countdown: Duration::from_secs(45),
            presets: [15, 60].map(Duration::from_mins).to_vec(),
            allow_custom: true,
            stale_outputs: vec![StaleOutput {
                output: "HDMI-A-1".into(),
                unchanged_percent: 84,
            }],
        }
    }

    #[test]
    fn menu_lists_the_same_actions_as_the_notification() {
        let request = request();
        let spec = menu_spec(&request);
        assert_eq!(spec.program, "kdialog");
        assert_eq!(spec.timeout, Duration::from_secs(75));
        assert_eq!(
            spec.args,
            [
                "--title",
                APP_ID,
                "--menu",
                "Blanking the screen in 45 s\n\
                 HDMI-A-1 has been static: 84% of the screen unchanged.\n\
                 Snooze to keep it on.",
                "snooze:15",
                "15 min",
                "snooze:60",
                "1 h",
                "custom",
                "Custom...",
                "blank-now",
                "Blank now",
            ]
        );
    }

    #[test]
    fn tags_and_cancel_map_to_outcomes() {
        let snooze = outcome_of(Ok(stdout("snooze:15\n"))).unwrap();
        assert_eq!(snooze, PromptOutcome::Snooze(Duration::from_mins(15)));
        assert_eq!(
            outcome_of(Ok(stdout("blank-now"))).unwrap(),
            PromptOutcome::Timeout
        );
        assert_eq!(
            outcome_of(Ok(stdout("custom"))).unwrap(),
            PromptOutcome::CustomRequested
        );
        let cancelled = CommandError::Failed {
            program: "kdialog".into(),
            code: Some(CANCELLED),
            stderr: String::new(),
        };
        assert_eq!(
            outcome_of(Err(cancelled)).unwrap(),
            PromptOutcome::Dismissed
        );
        assert!(outcome_of(Ok(stdout("nope"))).is_err());
        let missing = CommandError::NotFound {
            program: "kdialog".into(),
        };
        assert!(matches!(
            outcome_of(Err(missing)),
            Err(BackendError::Unavailable(_))
        ));
    }

    fn stdout(text: &str) -> CommandOutput {
        CommandOutput {
            stdout: text.to_owned(),
            stderr: String::new(),
        }
    }
}
