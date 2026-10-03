use evdev::AbsoluteAxisCode as Abs;
use proptest::prelude::*;

use super::*;

const DEADZONE: u8 = 15;

fn range(min: i32, max: i32, value: i32) -> AxisRange {
    AxisRange {
        min,
        max,
        flat: 0,
        value,
    }
}

fn stick(min: i32, max: i32) -> Axis {
    Axis::new(Abs::ABS_X.0, range(min, max, 0))
}

fn analog(min: i32, max: i32, flat: i32, rest: Rest) -> Axis {
    Axis::Analog {
        min,
        max,
        flat,
        rest,
    }
}

#[test]
fn signed_stick_ignores_drift_and_counts_real_movement() {
    let axis = stick(-32768, 32767);
    assert!(!axis.passes(0, DEADZONE));
    assert!(!axis.passes(1200, DEADZONE));
    assert!(!axis.passes(4914, DEADZONE));
    assert!(axis.passes(4915, DEADZONE));
    assert!(axis.passes(32767, DEADZONE));
}

#[test]
fn signed_stick_is_symmetric_on_the_negative_side() {
    let axis = stick(-32768, 32767);
    assert!(!axis.passes(-4915, DEADZONE));
    assert!(axis.passes(-4916, DEADZONE));
    assert!(axis.passes(-32768, DEADZONE));
}

#[test]
fn unsigned_stick_centers_between_two_values() {
    let axis = stick(0, 255);
    assert!(!axis.passes(127, DEADZONE));
    assert!(!axis.passes(128, DEADZONE));
    assert!(!axis.passes(146, DEADZONE));
    assert!(axis.passes(147, DEADZONE));
    assert!(!axis.passes(109, DEADZONE));
    assert!(axis.passes(108, DEADZONE));
}

#[test]
fn exactly_at_the_boundary_counts() {
    let axis = stick(0, 200);
    assert!(axis.passes(115, DEADZONE));
    assert!(axis.passes(85, DEADZONE));
    assert!(!axis.passes(114, DEADZONE));
    assert!(!axis.passes(86, DEADZONE));
}

#[test]
fn asymmetric_ranges_measure_from_their_own_center() {
    let axis = stick(-100, 300);
    assert!(!axis.passes(100, DEADZONE));
    assert!(axis.passes(130, DEADZONE));
    assert!(!axis.passes(129, DEADZONE));
    assert!(axis.passes(70, DEADZONE));
    assert!(!axis.passes(71, DEADZONE));
}

#[test]
fn negative_only_ranges_work() {
    let axis = stick(-300, -100);
    assert!(!axis.passes(-200, DEADZONE));
    assert!(axis.passes(-185, DEADZONE));
    assert!(!axis.passes(-186, DEADZONE));
    assert!(axis.passes(-215, DEADZONE));
}

#[test]
fn triggers_measure_from_their_resting_end() {
    let trigger = Axis::new(Abs::ABS_Z.0, range(0, 1023, 0));
    assert_eq!(trigger, analog(0, 1023, 0, Rest::Min));
    assert!(!trigger.passes(0, DEADZONE));
    assert!(!trigger.passes(153, DEADZONE));
    assert!(trigger.passes(154, DEADZONE));
    assert!(trigger.passes(1023, DEADZONE));
}

#[test]
fn a_trigger_below_its_minimum_is_at_rest() {
    let trigger = analog(0, 255, 0, Rest::Min);
    assert!(!trigger.passes(-50, 0));
}

#[test]
fn inverted_pedals_rest_at_max() {
    let pedal = Axis::new(Abs::ABS_RZ.0, range(0, 255, 255));
    assert_eq!(pedal, analog(0, 255, 0, Rest::Max));
    assert!(!pedal.passes(255, DEADZONE));
    assert!(!pedal.passes(220, DEADZONE));
    assert!(pedal.passes(216, DEADZONE));
    assert!(pedal.passes(0, DEADZONE));
    assert!(!pedal.passes(300, 0));
}

#[test]
fn a_centered_z_axis_is_treated_as_a_stick() {
    let right_stick = Axis::new(Abs::ABS_Z.0, range(0, 255, 128));
    assert_eq!(right_stick, analog(0, 255, 0, Rest::Center));
    assert!(!right_stick.passes(140, DEADZONE));
    assert!(right_stick.passes(160, DEADZONE));
}

#[test]
fn sticks_rest_at_center_even_when_held_at_open() {
    for code in [Abs::ABS_X, Abs::ABS_Y, Abs::ABS_RX, Abs::ABS_RY] {
        let axis = Axis::new(code.0, range(0, 255, 255));
        assert_eq!(axis, analog(0, 255, 0, Rest::Center), "{code:?}");
    }
}

#[test]
fn ties_between_rests_prefer_the_minimum() {
    assert_eq!(nearest_rest(range(0, 4, 1)), Rest::Min);
    assert_eq!(nearest_rest(range(0, 4, 3)), Rest::Center);
    assert_eq!(nearest_rest(range(0, 4, 4)), Rest::Max);
}

#[test]
fn hats_always_count() {
    for code in Abs::ABS_HAT0X.0..=Abs::ABS_HAT3Y.0 {
        let hat = Axis::new(code, range(-1, 1, 0));
        assert_eq!(hat, Axis::Hat);
        for value in [-1, 0, 1] {
            assert!(hat.passes(value, 100));
        }
    }
}

#[test]
fn the_driver_flat_zone_is_respected() {
    let axis = analog(0, 255, 40, Rest::Center);
    assert!(!axis.passes(147, 0));
    assert!(!axis.passes(167, 0));
    assert!(axis.passes(168, 0));
    let negative_flat = analog(0, 255, -10, Rest::Center);
    assert!(negative_flat.passes(128, 0));
}

#[test]
fn zero_deadzone_still_ignores_the_exact_rest_position() {
    let axis = stick(-100, 100);
    assert!(!axis.passes(0, 0));
    assert!(axis.passes(1, 0));
}

#[test]
fn deadzones_over_100_clamp_to_full_deflection() {
    let axis = stick(-100, 100);
    assert!(axis.passes(100, 255));
    assert!(axis.passes(-100, 255));
    assert!(!axis.passes(99, 255));
}

#[test]
fn degenerate_ranges_never_count() {
    assert!(!stick(5, 5).passes(5, 0));
    assert!(!stick(5, 5).passes(900, 0));
    assert!(!stick(10, -10).passes(0, 0));
}

#[test]
fn axes_count_buttons_relative_and_declared_axes_only() {
    let axes = Axes::new([
        (Abs::ABS_X.0, range(-32768, 32767, 0)),
        (Abs::ABS_HAT0Y.0, range(-1, 1, 0)),
    ]);
    assert!(axes.counts(PadInput::Button, DEADZONE));
    assert!(axes.counts(PadInput::Relative, DEADZONE));
    assert!(!axes.counts(PadInput::Other, DEADZONE));
    let small = PadInput::Absolute {
        code: Abs::ABS_X.0,
        value: 100,
    };
    let big = PadInput::Absolute {
        code: Abs::ABS_X.0,
        value: 20_000,
    };
    assert!(!axes.counts(small, DEADZONE));
    assert!(axes.counts(big, DEADZONE));
    let hat = PadInput::Absolute {
        code: Abs::ABS_HAT0Y.0,
        value: -1,
    };
    assert!(axes.counts(hat, DEADZONE));
    let undeclared = PadInput::Absolute {
        code: Abs::ABS_RX.0,
        value: 20_000,
    };
    assert!(!axes.counts(undeclared, DEADZONE));
    assert!(!Axes::default().counts(big, 0));
}

fn any_rest() -> impl Strategy<Value = Rest> {
    prop_oneof![Just(Rest::Center), Just(Rest::Min), Just(Rest::Max)]
}

proptest! {
    #[test]
    fn never_panics(
        min in any::<i32>(),
        max in any::<i32>(),
        flat in any::<i32>(),
        value in any::<i32>(),
        rest in any_rest(),
        deadzone in any::<u8>(),
    ) {
        let _ = analog(min, max, flat, rest).passes(value, deadzone);
        let _ = Axis::new(Abs::ABS_Z.0, AxisRange { min, max, flat, value });
    }

    #[test]
    fn the_resting_end_never_counts(
        min in -100_000i32..100_000,
        width in 1i32..100_000,
        deadzone in any::<u8>(),
    ) {
        let max = min + width;
        prop_assert!(!analog(min, max, 0, Rest::Min).passes(min, deadzone));
        prop_assert!(!analog(min, max, 0, Rest::Max).passes(max, deadzone));
    }

    #[test]
    fn a_larger_deadzone_never_counts_more(
        min in -100_000i32..100_000,
        width in 1i32..100_000,
        offset in 0i32..100_000,
        flat in 0i32..1_000,
        rest in any_rest(),
        low in 0u8..=100,
        extra in 0u8..=100,
    ) {
        let axis = analog(min, min + width, flat, rest);
        let value = min + offset % (width + 1);
        let high = low.saturating_add(extra);
        if axis.passes(value, high) {
            prop_assert!(axis.passes(value, low));
        }
    }

    #[test]
    fn centered_axes_are_mirror_symmetric(
        min in -100_000i32..100_000,
        width in 1i32..100_000,
        offset in 0i32..100_000,
        deadzone in 0u8..=100,
    ) {
        let max = min + width;
        let axis = analog(min, max, 0, Rest::Center);
        let value = min + offset % (width + 1);
        let mirrored = min + max - value;
        prop_assert_eq!(axis.passes(value, deadzone), axis.passes(mirrored, deadzone));
    }

    #[test]
    fn full_deflection_always_counts(
        min in -100_000i32..100_000,
        width in 1i32..100_000,
        deadzone in any::<u8>(),
    ) {
        let max = min + width;
        let center = analog(min, max, 0, Rest::Center);
        prop_assert!(center.passes(min, deadzone));
        prop_assert!(center.passes(max, deadzone));
        prop_assert!(analog(min, max, 0, Rest::Min).passes(max, deadzone));
        prop_assert!(analog(min, max, 0, Rest::Max).passes(min, deadzone));
    }
}
