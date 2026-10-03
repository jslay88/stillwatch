//! Whether a node udev tags as a joystick really is one. Pure: works on the
//! capability sets alone.
//!
//! udev's `ID_INPUT_JOYSTICK` fires on any hat or extra absolute axis, which
//! catches keyboard media-key interfaces too (a "System Control" node with
//! `ABS_HAT0X`/`ABS_HAT0Y`/`ABS_MISC` and no joystick buttons). Pedals and
//! rudders are the opposite: axes in `ABS_X..=ABS_BRAKE` and no keys. A
//! button box is joystick buttons and no keyboard keys. A uinput device that
//! declares every key (stream-controller) still is not a gamepad.

use std::fmt;
use std::ops::RangeInclusive;

/// The `BTN_JOYSTICK` (0x120) and `BTN_GAMEPAD` (0x130) blocks. HID maps a
/// gamepad's 16th button to 0x13f, one past `BTN_THUMBR`.
const JOYSTICK_BUTTONS: RangeInclusive<u16> = 0x120..=0x13f;

/// `ABS_X` through `ABS_BRAKE`. Hats start at `ABS_HAT0X` (0x10) and
/// `ABS_MISC` is 0x28, so neither is in this range.
const ANALOG_AXES: RangeInclusive<u16> = 0x00..=0x0a;

/// Keys below `BTN_MISC` (0x100). The stream-controller device sets these
/// next to a joystick button; a button box does not.
const KEYBOARD_KEYS: RangeInclusive<u16> = 0x000..=0x0ff;

/// Whether an opened node gets treated as a gamepad.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// A joystick, gamepad, pedal set, or button box.
    Accept,
    /// Something else wearing the joystick tag.
    Reject(Reason),
}

/// Why a node isn't a gamepad.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reason {
    /// No joystick button, and no buttonless analog axis.
    NoButtons,
    /// Joystick buttons mixed with keyboard keys, and no absolute axis.
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

/// Whether `keys` and `axes` belong to a gamepad.
///
/// A buttonless node with an axis in `ABS_X..=ABS_BRAKE` is a pedal or
/// rudder. Hats and `ABS_MISC` do not count, and any declared key cancels
/// that exception. Otherwise a joystick button (`0x120..=0x13f`) is required:
/// with any absolute axis (hats count), or with no axes and no keyboard keys
/// (a button box). Joystick buttons mixed with keyboard keys and no axes are
/// rejected.
pub(crate) fn check(
    keys: impl IntoIterator<Item = u16>,
    axes: impl IntoIterator<Item = u16>,
) -> Verdict {
    let mut keys_present = false;
    let mut joystick_button = false;
    let mut keyboard_key = false;
    for key in keys {
        keys_present = true;
        joystick_button |= JOYSTICK_BUTTONS.contains(&key);
        keyboard_key |= KEYBOARD_KEYS.contains(&key);
    }
    let mut analog = false;
    let mut any_axis = false;
    for axis in axes {
        any_axis = true;
        analog |= ANALOG_AXES.contains(&axis);
    }

    let pedals = analog && !keys_present;
    let pad = joystick_button && any_axis;
    let button_box = joystick_button && !any_axis && !keyboard_key;
    if pedals || pad || button_box {
        Verdict::Accept
    } else if joystick_button {
        Verdict::Reject(Reason::NoAxes)
    } else {
        Verdict::Reject(Reason::NoButtons)
    }
}

#[cfg(test)]
mod tests;
