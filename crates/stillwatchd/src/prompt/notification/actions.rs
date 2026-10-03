//! The prompt's action buttons and what each one answers.
//!
//! Action keys are `snooze:<minutes>` per preset, `custom`, and `blank-now`.

use std::time::Duration;

use stillwatch_core::prompt::{PromptOutcome, PromptRequest};

use super::text::duration_label;

/// Opens `stillwatch-gui prompt`, which answers over D-Bus `PromptAnswer`.
pub(crate) const CUSTOM: &str = "custom";

/// Acts right away instead of waiting for the countdown.
pub(crate) const BLANK_NOW: &str = "blank-now";

const SNOOZE_PREFIX: &str = "snooze:";

const CUSTOM_LABEL: &str = "Custom...";
const BLANK_NOW_LABEL: &str = "Blank now";

/// The `actions` argument of `Notify`: key and label pairs, flattened, in
/// button order (presets, then "Custom...", then "Blank now").
pub(crate) fn prompt_actions(request: &PromptRequest) -> Vec<String> {
    let mut actions = Vec::new();
    for preset in &request.presets {
        let minutes = preset.as_secs() / 60;
        actions.push(format!("{SNOOZE_PREFIX}{minutes}"));
        actions.push(duration_label(Duration::from_mins(minutes)));
    }
    if request.allow_custom {
        actions.extend([CUSTOM.to_owned(), CUSTOM_LABEL.to_owned()]);
    }
    actions.extend([BLANK_NOW.to_owned(), BLANK_NOW_LABEL.to_owned()]);
    actions
}

/// What an invoked action key answers. `None` for keys Stillwatch didn't
/// offer, such as `default` when the notification body is clicked.
///
/// "Blank now" answers [`PromptOutcome::Timeout`]: the state machine acts on
/// it exactly as when the countdown runs out.
pub(crate) fn outcome(key: &str) -> Option<PromptOutcome> {
    match key {
        CUSTOM => Some(PromptOutcome::CustomRequested),
        BLANK_NOW => Some(PromptOutcome::Timeout),
        _ => {
            let minutes = key.strip_prefix(SNOOZE_PREFIX)?.parse().ok()?;
            Some(PromptOutcome::Snooze(Duration::from_mins(minutes)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(presets: &[u64], allow_custom: bool) -> PromptRequest {
        PromptRequest {
            countdown: Duration::from_mins(1),
            presets: presets.iter().copied().map(Duration::from_mins).collect(),
            allow_custom,
            stale_outputs: vec![],
        }
    }

    #[test]
    fn default_presets_then_custom_then_blank_now() {
        assert_eq!(
            prompt_actions(&request(&[15, 60, 180], true)),
            [
                "snooze:15",
                "15 min",
                "snooze:60",
                "1 h",
                "snooze:180",
                "3 h",
                "custom",
                "Custom...",
                "blank-now",
                "Blank now",
            ]
        );
    }

    #[test]
    fn custom_is_left_out_when_not_allowed() {
        assert_eq!(
            prompt_actions(&request(&[90], false)),
            ["snooze:90", "1 h 30 min", "blank-now", "Blank now"]
        );
        assert_eq!(
            prompt_actions(&request(&[], true)),
            ["custom", "Custom...", "blank-now", "Blank now"]
        );
    }

    #[test]
    fn every_offered_key_maps_back_to_an_outcome() {
        let actions = prompt_actions(&request(&[15, 60], true));
        let outcomes: Vec<_> = actions.iter().step_by(2).map(|key| outcome(key)).collect();
        assert_eq!(
            outcomes,
            [
                Some(PromptOutcome::Snooze(Duration::from_mins(15))),
                Some(PromptOutcome::Snooze(Duration::from_hours(1))),
                Some(PromptOutcome::CustomRequested),
                Some(PromptOutcome::Timeout),
            ]
        );
    }

    #[test]
    fn unknown_keys_answer_nothing() {
        for key in [
            "default",
            "",
            "snooze:",
            "snooze:soon",
            "snooze:-5",
            "cancel",
        ] {
            assert_eq!(outcome(key), None, "{key:?}");
        }
    }
}
