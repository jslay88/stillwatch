use super::State;
use super::State::{Acting, Active, Blanked, Locked, Monitoring, Paused, Prompting, Snoozed};

/// One documented edge of the state diagram.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransitionRule {
    /// State before.
    pub from: State,
    /// State after.
    pub to: State,
    /// What causes it, as written in the State machine document.
    pub trigger: &'static str,
}

const fn rule(from: State, to: State, trigger: &'static str) -> TransitionRule {
    TransitionRule { from, to, trigger }
}

/// Every transition the [`StateMachine`](super::StateMachine) may take.
///
/// The machine refuses any transition missing from this table, so adding a
/// behavior means adding its row here and to the State machine document.
pub const TRANSITIONS: &[TransitionRule] = &[
    rule(Active, Monitoring, "idle"),
    rule(Active, Snoozed, "snooze command"),
    rule(Active, Locked, "session locked"),
    rule(Monitoring, Active, "input"),
    rule(Monitoring, Prompting, "stale"),
    rule(Monitoring, Snoozed, "snooze command"),
    rule(Monitoring, Locked, "session locked"),
    rule(Prompting, Snoozed, "snooze"),
    rule(Prompting, Active, "input or cancel"),
    rule(Prompting, Acting, "timeout"),
    rule(Acting, Blanked, "action completed"),
    rule(Acting, Active, "input, or action failed while active"),
    rule(Acting, Monitoring, "action failed while idle"),
    rule(Blanked, Active, "input"),
    rule(Snoozed, Monitoring, "cancelled or expired while idle"),
    rule(
        Snoozed,
        Active,
        "cancelled or expired while active, or input",
    ),
    rule(Locked, Active, "unlocked"),
    rule(Active, Paused, "pause"),
    rule(Monitoring, Paused, "pause"),
    rule(Prompting, Paused, "pause"),
    rule(Snoozed, Paused, "pause"),
    rule(Acting, Paused, "pause"),
    rule(Blanked, Paused, "pause"),
    rule(Locked, Paused, "pause"),
    rule(Paused, Monitoring, "resumed while idle"),
    rule(Paused, Active, "resumed while active"),
];

/// The table row for `from -> to`, if that transition is documented.
#[must_use]
pub fn rule_for(from: State, to: State) -> Option<&'static TransitionRule> {
    TRANSITIONS
        .iter()
        .find(|rule| rule.from == from && rule.to == to)
}
