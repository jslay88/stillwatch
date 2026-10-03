//! Merge the form's lists with the daemon's live devices.
//!
//! Configured entries that aren't in the live snapshot stay in the list and
//! are marked not connected, so a missing daemon or an unplugged device
//! doesn't drop what the file already says. Free text still appends through
//! the normal list editor.

use stillwatch_core::config::{StaleConfig, ignores_device_name};

use super::catalog::{self, GamepadSeen, PlayerSeen};

/// Shown beside a configured entry the daemon doesn't currently report.
pub const NOT_CONNECTED: &str = "not connected";

/// Hint under the output picker while nothing is selected.
pub const WATCHES_ALL: &str = "None selected watches every connected output.";

/// One checkbox in an output, gamepad, or player picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickerRow {
    /// Text written into the config when the row is checked.
    pub value: String,
    /// What the row shows. Players use the stable name, not the instance suffix.
    pub label: String,
    /// Whether the form's list already includes this entry.
    pub selected: bool,
    /// `false` when the entry is configured but not in the live snapshot.
    pub connected: bool,
    /// `Some` for a live gamepad: `true` while its last event is inside the
    /// activity pulse window.
    pub activity: Option<bool>,
}

impl PickerRow {
    /// [`NOT_CONNECTED`] when this entry isn't in the daemon's live list.
    #[must_use]
    pub fn absence(&self) -> Option<&'static str> {
        if self.connected {
            None
        } else {
            Some(NOT_CONNECTED)
        }
    }

    /// [`catalog::ACTIVITY_LABEL`] while a live gamepad is pulsing.
    #[must_use]
    pub fn activity_label(&self) -> Option<&'static str> {
        self.activity
            .filter(|lit| *lit)
            .map(|_| catalog::ACTIVITY_LABEL)
    }
}

/// The name to store for an MPRIS player.
///
/// Instance suffixes change every launch (`firefox.instance_1_42`), and
/// [`StaleConfig::is_player_ignored`] already treats the part before
/// `.instance` as the player. The picker writes that part, not `Identity`.
#[must_use]
pub fn player_value(name: &str) -> &str {
    name.split_once(".instance").map_or(name, |(head, _)| head)
}

/// Suffix, with `Identity` beside it when the player reported one.
#[must_use]
pub fn player_label(name: &str, identity: &str) -> String {
    let stem = player_value(name);
    if identity.is_empty() {
        stem.to_owned()
    } else {
        format!("{stem} · {identity}")
    }
}

/// Connected outputs, then configured names that aren't connected.
///
/// An empty `configured` list selects nothing. Empty means every output.
#[must_use]
pub fn output_rows(configured: &[String], live: &[String]) -> Vec<PickerRow> {
    let mut rows = live
        .iter()
        .map(|name| {
            row(
                name,
                name,
                configured.iter().any(|item| item == name),
                true,
                None,
            )
        })
        .collect::<Vec<_>>();
    for name in configured {
        if !live.iter().any(|item| item == name) {
            rows.push(row(name, name, true, false, None));
        }
    }
    rows
}

/// Detected gamepads, with activity, then ignore entries that match none of them.
///
/// A row is selected when any configured substring matches the device name.
/// Checking stores the device name itself.
#[must_use]
pub fn gamepad_rows(configured: &[String], live: &[GamepadSeen]) -> Vec<PickerRow> {
    let mut rows = live
        .iter()
        .map(|pad| {
            row(
                &pad.name,
                &pad.name,
                ignores_device_name(configured, &pad.name),
                true,
                Some(catalog::activity_lit(pad.seconds_since_activity)),
            )
        })
        .collect::<Vec<_>>();
    for needle in configured {
        let matched = live
            .iter()
            .any(|pad| ignores_device_name(std::slice::from_ref(needle), &pad.name));
        if !matched {
            rows.push(row(needle, needle, true, false, None));
        }
    }
    rows
}

/// Current players, one row per stable name, then configured names that match none.
///
/// The label shows `Identity` beside the suffix. Checking still stores the suffix.
#[must_use]
pub fn player_rows(configured: &[String], live: &[PlayerSeen]) -> Vec<PickerRow> {
    let mut rows = Vec::new();
    let mut seen = Vec::new();
    for player in live {
        let stem = player_value(&player.name);
        if seen
            .iter()
            .any(|have: &String| have.eq_ignore_ascii_case(stem))
        {
            continue;
        }
        seen.push(stem.to_owned());
        let selected = configured.iter().any(|entry| {
            live.iter().any(|candidate| {
                player_value(&candidate.name).eq_ignore_ascii_case(stem)
                    && entry_covers(entry, candidate)
            })
        });
        rows.push(row(
            stem,
            &player_label(&player.name, &player.identity),
            selected,
            true,
            None,
        ));
    }
    for entry in configured {
        if !live.iter().any(|player| entry_covers(entry, player)) {
            rows.push(row(entry, entry, true, false, None));
        }
    }
    rows
}

/// Add or remove `value` as an exact list entry.
#[must_use]
pub fn set_exact(items: &[String], value: &str, on: bool) -> Vec<String> {
    let mut next = items
        .iter()
        .filter(|item| item.as_str() != value)
        .cloned()
        .collect::<Vec<_>>();
    if on {
        next.push(value.to_owned());
    }
    next
}

/// Checking adds `name`. Unchecking removes every substring that matches it.
#[must_use]
pub fn set_gamepad(items: &[String], name: &str, on: bool) -> Vec<String> {
    if on {
        if ignores_device_name(items, name) {
            return items.to_vec();
        }
        let mut next = items.to_vec();
        next.push(name.to_owned());
        return next;
    }
    items
        .iter()
        .filter(|needle| !ignores_device_name(std::slice::from_ref(needle), name))
        .cloned()
        .collect()
}

/// Checking adds the stable player name. Unchecking removes entries that match it.
#[must_use]
pub fn set_player(items: &[String], value: &str, live: &[PlayerSeen], on: bool) -> Vec<String> {
    if on {
        if items.iter().any(|item| item.eq_ignore_ascii_case(value)) {
            return items.to_vec();
        }
        let mut next = items.to_vec();
        next.push(value.to_owned());
        return next;
    }
    let covered: Vec<&PlayerSeen> = live
        .iter()
        .filter(|player| {
            player_value(&player.name).eq_ignore_ascii_case(value)
                || player.name.eq_ignore_ascii_case(value)
        })
        .collect();
    items
        .iter()
        .filter(|item| {
            if item.eq_ignore_ascii_case(value) {
                return false;
            }
            !covered.iter().any(|player| entry_covers(item, player))
        })
        .cloned()
        .collect()
}

fn row(
    value: &str,
    label: &str,
    selected: bool,
    connected: bool,
    activity: Option<bool>,
) -> PickerRow {
    PickerRow {
        value: value.to_owned(),
        label: label.to_owned(),
        selected,
        connected,
        activity,
    }
}

fn entry_covers(entry: &str, player: &PlayerSeen) -> bool {
    let stale = StaleConfig {
        media_ignore_players: vec![entry.to_owned()],
        ..StaleConfig::default()
    };
    stale.is_player_ignored(&player.name, &player.identity)
}

#[cfg(test)]
mod tests;
