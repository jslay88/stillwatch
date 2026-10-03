use evdev::{AbsoluteAxisCode as Abs, KeyCode as Key};

use super::*;

/// Decodes a `B: KEY=` / `B: ABS=` bitmap from `/proc/bus/input/devices`:
/// space-separated 64-bit hex words, most significant first.
fn bitmap(words: &str) -> Vec<u16> {
    let words: Vec<u64> = words
        .split_whitespace()
        .rev()
        .map(|word| u64::from_str_radix(word, 16).unwrap())
        .collect();
    let mut codes = Vec::new();
    for (index, word) in words.iter().enumerate() {
        for bit in 0..64 {
            if word >> bit & 1 == 1 {
                codes.push(u16::try_from(index * 64 + bit).unwrap());
            }
        }
    }
    codes
}

fn keys(codes: &[Key]) -> Vec<u16> {
    codes.iter().map(|code| code.0).collect()
}

fn axes(codes: &[Abs]) -> Vec<u16> {
    codes.iter().map(|code| code.0).collect()
}

const FACE: [Key; 4] = [Key::BTN_SOUTH, Key::BTN_EAST, Key::BTN_NORTH, Key::BTN_WEST];
const STICKS_AND_HAT: [Abs; 6] = [
    Abs::ABS_X,
    Abs::ABS_Y,
    Abs::ABS_RX,
    Abs::ABS_RY,
    Abs::ABS_HAT0X,
    Abs::ABS_HAT0Y,
];

#[test]
fn bitmap_decodes_proc_words() {
    assert_eq!(bitmap("10000030000"), [0x10, 0x11, 0x28]);
    assert_eq!(bitmap("1 0"), [64]);
    assert_eq!(bitmap("0"), Vec::<u16>::new());
}

#[test]
fn keychron_system_control_is_rejected() {
    // "Keychron Keychron K5 Version 2 System Control", event17 on the dev box.
    let keys = bitmap(
        "c000 0 0 40000001000000 1200000000 0 100000800000000 40000010cc00 10168000000000 0",
    );
    let axes = bitmap("10000030000");
    assert!(keys.contains(&Key::KEY_POWER.0));
    assert_eq!(axes, [Abs::ABS_HAT0X.0, Abs::ABS_HAT0Y.0, Abs::ABS_MISC.0]);
    assert_eq!(check(keys, axes), Verdict::Reject(Reason::NoButtons));
}

#[test]
fn xbox_pad_is_accepted() {
    // xpad's Xbox One / Series controller.
    let keys = bitmap("7cdb000000000000 0 0 0 0");
    assert_eq!(
        keys,
        self::keys(&[
            Key::BTN_SOUTH,
            Key::BTN_EAST,
            Key::BTN_NORTH,
            Key::BTN_WEST,
            Key::BTN_TL,
            Key::BTN_TR,
            Key::BTN_SELECT,
            Key::BTN_START,
            Key::BTN_MODE,
            Key::BTN_THUMBL,
            Key::BTN_THUMBR,
        ])
    );
    assert_eq!(check(keys, bitmap("3003f")), Verdict::Accept);
}

#[test]
fn dualsense_is_accepted() {
    let mut buttons = FACE.to_vec();
    buttons.extend([
        Key::BTN_TL,
        Key::BTN_TR,
        Key::BTN_TL2,
        Key::BTN_TR2,
        Key::BTN_SELECT,
        Key::BTN_START,
        Key::BTN_MODE,
        Key::BTN_THUMBL,
        Key::BTN_THUMBR,
    ]);
    let mut sticks = STICKS_AND_HAT.to_vec();
    sticks.extend([Abs::ABS_Z, Abs::ABS_RZ]);
    assert_eq!(check(keys(&buttons), axes(&sticks)), Verdict::Accept);
}

#[test]
fn switch_pro_is_accepted() {
    let mut buttons = FACE.to_vec();
    buttons.extend([
        Key::BTN_TL,
        Key::BTN_TR,
        Key::BTN_TL2,
        Key::BTN_TR2,
        Key::BTN_SELECT,
        Key::BTN_START,
        Key::BTN_MODE,
        Key::BTN_Z,
        Key::BTN_THUMBL,
        Key::BTN_THUMBR,
        Key::BTN_DPAD_UP,
        Key::BTN_DPAD_DOWN,
        Key::BTN_DPAD_LEFT,
        Key::BTN_DPAD_RIGHT,
    ]);
    assert_eq!(
        check(keys(&buttons), axes(&STICKS_AND_HAT[..4])),
        Verdict::Accept
    );
}

#[test]
fn flight_stick_is_accepted() {
    assert_eq!(
        check(keys(&[Key::BTN_TRIGGER]), axes(&[Abs::ABS_X, Abs::ABS_Y])),
        Verdict::Accept
    );
}

#[test]
fn a_generic_pads_sixteenth_button_counts() {
    assert_eq!(check([0x13f], axes(&[Abs::ABS_X])), Verdict::Accept);
    assert_eq!(
        check([0x11f, 0x140], axes(&[Abs::ABS_X])),
        Verdict::Reject(Reason::NoButtons)
    );
}

#[test]
fn hat_only_device_without_buttons_is_rejected() {
    assert_eq!(
        check([], axes(&[Abs::ABS_HAT0X, Abs::ABS_HAT0Y])),
        Verdict::Reject(Reason::NoButtons)
    );
}

#[test]
fn buttons_without_axes_are_rejected() {
    // "stream-controller-os-plugin", a uinput device that declares every key.
    let keys =
        bitmap("fffffffffff ffffffffffffffff ffffffffffffffff ffffffffffffffff fffffffffffffffe");
    assert!(keys.contains(&Key::BTN_TRIGGER.0));
    assert_eq!(check(keys, []), Verdict::Reject(Reason::NoAxes));
}

#[test]
fn reasons_read_as_log_text() {
    assert_eq!(
        Reason::NoButtons.to_string(),
        "no joystick or gamepad buttons"
    );
    assert_eq!(Reason::NoAxes.to_string(), "no absolute axes");
}
