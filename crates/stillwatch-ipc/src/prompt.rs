//! Encoding of `PromptAnswer(kind, minutes)`, sent by `stillwatch-gui prompt`.

use std::fmt;
use std::str::FromStr;
use std::time::Duration;

use stillwatch_core::prompt::PromptOutcome;

use crate::error::IpcError;

/// The `kind` argument of `PromptAnswer`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PromptAnswerKind {
    /// `"snooze"`: snooze for `minutes` (must be at least 1).
    Snooze,
    /// `"cancel"`: the user is present; don't act.
    Cancel,
    /// `"timeout"`: the dialog's countdown ran out.
    Timeout,
    /// `"dismissed"`: the dialog was closed without an answer.
    Dismissed,
}

impl PromptAnswerKind {
    const ALL: [Self; 4] = [Self::Snooze, Self::Cancel, Self::Timeout, Self::Dismissed];

    /// The wire name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Snooze => "snooze",
            Self::Cancel => "cancel",
            Self::Timeout => "timeout",
            Self::Dismissed => "dismissed",
        }
    }
}

impl fmt::Display for PromptAnswerKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for PromptAnswerKind {
    type Err = IpcError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.as_str() == s)
            .ok_or_else(|| IpcError::PromptAnswer(format!("unknown kind {s:?}")))
    }
}

/// Decodes `PromptAnswer(kind, minutes)` into an outcome. `minutes` is only
/// read for `"snooze"`.
///
/// # Errors
///
/// Returns [`IpcError::PromptAnswer`] for an unknown kind or a zero-minute
/// snooze.
pub fn outcome_from_answer(kind: &str, minutes: u32) -> Result<PromptOutcome, IpcError> {
    Ok(match kind.parse()? {
        PromptAnswerKind::Snooze if minutes == 0 => {
            return Err(IpcError::PromptAnswer(
                "snooze needs at least 1 minute".into(),
            ));
        }
        PromptAnswerKind::Snooze => {
            PromptOutcome::Snooze(Duration::from_secs(u64::from(minutes) * 60))
        }
        PromptAnswerKind::Cancel => PromptOutcome::Cancel,
        PromptAnswerKind::Timeout => PromptOutcome::Timeout,
        PromptAnswerKind::Dismissed => PromptOutcome::Dismissed,
    })
}

/// Encodes an outcome as `PromptAnswer` arguments.
///
/// Returns `None` for outcomes the wire can't carry: `CustomRequested`, and
/// snoozes that aren't a whole number of minutes between 1 and `u32::MAX`.
#[must_use]
pub fn answer_from_outcome(outcome: PromptOutcome) -> Option<(PromptAnswerKind, u32)> {
    match outcome {
        PromptOutcome::Snooze(duration) => {
            let secs = duration.as_secs();
            let whole = secs % 60 == 0 && duration.subsec_nanos() == 0;
            let minutes = u32::try_from(secs / 60).ok().filter(|m| *m > 0 && whole)?;
            Some((PromptAnswerKind::Snooze, minutes))
        }
        PromptOutcome::Cancel => Some((PromptAnswerKind::Cancel, 0)),
        PromptOutcome::Timeout => Some((PromptAnswerKind::Timeout, 0)),
        PromptOutcome::Dismissed => Some((PromptAnswerKind::Dismissed, 0)),
        PromptOutcome::CustomRequested => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_carriable_outcome_round_trips() {
        let outcomes = [
            PromptOutcome::Snooze(Duration::from_mins(45)),
            PromptOutcome::Cancel,
            PromptOutcome::Timeout,
            PromptOutcome::Dismissed,
        ];
        for outcome in outcomes {
            let (kind, minutes) = answer_from_outcome(outcome).unwrap();
            assert_eq!(
                outcome_from_answer(kind.as_str(), minutes).unwrap(),
                outcome
            );
            assert_eq!(kind.to_string(), kind.as_str());
        }
    }

    #[test]
    fn uncarriable_outcomes_have_no_encoding() {
        assert_eq!(answer_from_outcome(PromptOutcome::CustomRequested), None);
        assert_eq!(
            answer_from_outcome(PromptOutcome::Snooze(Duration::from_secs(90))),
            None
        );
        assert_eq!(
            answer_from_outcome(PromptOutcome::Snooze(Duration::ZERO)),
            None
        );
        let fractional = Duration::from_mins(1) + Duration::from_millis(1);
        assert_eq!(answer_from_outcome(PromptOutcome::Snooze(fractional)), None);
    }

    #[test]
    fn bad_answers_are_rejected() {
        let err = outcome_from_answer("later", 5).unwrap_err();
        assert_eq!(
            err.to_string(),
            r#"invalid prompt answer: unknown kind "later""#
        );
        assert!(outcome_from_answer("snooze", 0).is_err());
        assert_eq!(
            outcome_from_answer("cancel", 99).unwrap(),
            PromptOutcome::Cancel
        );
    }
}
