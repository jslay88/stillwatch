//! Whether a node udev tags as a joystick really is one. Pure: works on the
//! capability sets alone.
//!
//! udev's `ID_INPUT_JOYSTICK` fires on any hat or extra absolute axis, which
//! catches keyboard media-key interfaces too (a "System Control" node with
//! `ABS_HAT0X`/`ABS_HAT0Y`/`ABS_MISC` and no joystick buttons).

use std::fmt;
use std::ops::RangeInclusive;

/// The `BTN_JOYSTICK` (0x120) and `BTN_GAMEPAD` (0x130) blocks. HID maps a
/// gamepad's 16th button to 0x13f, one past `BTN_THUMBR`.
const JOYSTICK_BUTTONS: RangeInclusive<u16> = 0x120..=0x13f;

/// Whether an opened node gets treated as a gamepad.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// A joystick or gamepad.
    Accept,
    /// Something else wearing the joystick tag.
    Reject(Reason),
}

/// Why a node isn't a gamepad.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reason {
    /// No key in the joystick or gamepad button blocks.
    NoButtons,
    /// No absolute axis or hat.
    NoAxes,
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NoButtons => "no joystick or gamepad buttons",
            Self::NoAxes => "no absolute axes",
        })
    }
}

/// Accepts a node with at least one joystick or gamepad button and at least
/// one absolute axis (hats count), given the key and absolute axis codes it
/// declares.
pub(crate) fn check(
    keys: impl IntoIterator<Item = u16>,
    axes: impl IntoIterator<Item = u16>,
) -> Verdict {
    if !keys.into_iter().any(|key| JOYSTICK_BUTTONS.contains(&key)) {
        Verdict::Reject(Reason::NoButtons)
    } else if axes.into_iter().next().is_none() {
        Verdict::Reject(Reason::NoAxes)
    } else {
        Verdict::Accept
    }
}

#[cfg(test)]
mod tests;
