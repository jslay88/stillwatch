//! Deadzone math. Pure: no devices, no clock.
//!
//! Every comparison runs on doubled `i64` coordinates, so the center of an
//! odd-width range (0..=255 centers on 127.5) is exact and a value sitting
//! exactly on the deadzone boundary always counts.

use std::collections::HashMap;

use evdev::AbsoluteAxisCode;

/// One absolute axis as reported by `EVIOCGABS` when the device was opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AxisRange {
    pub(crate) min: i32,
    pub(crate) max: i32,
    /// The driver's own center deadzone, in axis units.
    pub(crate) flat: i32,
    /// The axis position at open time, used to tell where it rests.
    pub(crate) value: i32,
}

/// Where an analog axis sits when nobody touches it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Rest {
    /// Sticks and wheels: spring back to the middle.
    Center,
    /// Triggers and most pedals.
    Min,
    /// Inverted pedals.
    Max,
}

/// How an absolute axis is judged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Axis {
    /// D-pad hats report -1/0/1, so every change counts.
    Hat,
    /// Counts once its deflection from rest reaches the deadzone.
    Analog {
        min: i32,
        max: i32,
        flat: i32,
        rest: Rest,
    },
}

const STICKS: [AbsoluteAxisCode; 4] = [
    AbsoluteAxisCode::ABS_X,
    AbsoluteAxisCode::ABS_Y,
    AbsoluteAxisCode::ABS_RX,
    AbsoluteAxisCode::ABS_RY,
];

fn is_hat(code: u16) -> bool {
    (AbsoluteAxisCode::ABS_HAT0X.0..=AbsoluteAxisCode::ABS_HAT3Y.0).contains(&code)
}

impl Axis {
    /// Classifies `code`. Stick axes always rest at center. Anything else
    /// (triggers, pedals, a generic pad's Z/RZ right stick) rests wherever it
    /// was nearest to at open time: min, center, or max.
    pub(crate) fn new(code: u16, range: AxisRange) -> Self {
        if is_hat(code) {
            return Self::Hat;
        }
        let rest = if STICKS.iter().any(|stick| stick.0 == code) {
            Rest::Center
        } else {
            nearest_rest(range)
        };
        Self::Analog {
            min: range.min,
            max: range.max,
            flat: range.flat,
            rest,
        }
    }

    /// Whether moving to `value` counts as input at this deadzone.
    pub(crate) fn passes(self, value: i32, deadzone_percent: u8) -> bool {
        let Self::Analog {
            min,
            max,
            flat,
            rest,
        } = self
        else {
            return true;
        };
        let Some((distance, span)) = deflection(value, min, max, rest) else {
            return false;
        };
        let deadzone = i64::from(deadzone_percent.min(100));
        let flat = 2 * i64::from(flat.max(0));
        distance > flat && distance * 100 >= deadzone * span
    }
}

/// Doubled distance from rest and the doubled span it is measured against,
/// or `None` for a degenerate range.
fn deflection(value: i32, min: i32, max: i32, rest: Rest) -> Option<(i64, i64)> {
    let (value, min, max) = (i64::from(value), i64::from(min), i64::from(max));
    if max <= min {
        return None;
    }
    let full = 2 * (max - min);
    Some(match rest {
        Rest::Center => ((2 * value - (min + max)).abs(), max - min),
        Rest::Min => ((2 * (value - min)).max(0), full),
        Rest::Max => ((2 * (max - value)).max(0), full),
    })
}

fn nearest_rest(range: AxisRange) -> Rest {
    let (value, min, max) = (
        i64::from(range.value),
        i64::from(range.min),
        i64::from(range.max),
    );
    let candidates = [
        (Rest::Min, (2 * (value - min)).abs()),
        (Rest::Center, (2 * value - (min + max)).abs()),
        (Rest::Max, (2 * (max - value)).abs()),
    ];
    candidates
        .into_iter()
        .min_by_key(|&(_, distance)| distance)
        .map_or(Rest::Center, |(rest, _)| rest)
}

/// One evdev event, reduced to what matters for activity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PadInput {
    /// A button press or release.
    Button,
    /// A relative axis (trackball, scroll wheel) moved.
    Relative,
    /// An absolute axis moved to `value`.
    Absolute { code: u16, value: i32 },
    /// Sync, misc, LEDs, force feedback: never input.
    Other,
}

/// The absolute axes of one device, captured when it was opened.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Axes(HashMap<u16, Axis>);

impl Axes {
    pub(crate) fn new(ranges: impl IntoIterator<Item = (u16, AxisRange)>) -> Self {
        Self(
            ranges
                .into_iter()
                .map(|(code, range)| (code, Axis::new(code, range)))
                .collect(),
        )
    }

    /// Whether `input` counts as gamepad activity. Buttons, relative axes,
    /// and hats always do; analog axes only past the deadzone; axes the
    /// device never declared don't.
    pub(crate) fn counts(&self, input: PadInput, deadzone_percent: u8) -> bool {
        match input {
            PadInput::Button | PadInput::Relative => true,
            PadInput::Absolute { code, value } => self
                .0
                .get(&code)
                .is_some_and(|axis| axis.passes(value, deadzone_percent)),
            PadInput::Other => false,
        }
    }
}

#[cfg(test)]
mod tests;
