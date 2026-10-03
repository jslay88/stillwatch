use super::*;

/// The value of binary16 `bits`, decoded the slow way.
fn half_value(bits: u16) -> f64 {
    let sign = if bits & 0x8000 == 0 { 1.0 } else { -1.0 };
    let exponent = i32::from((bits >> 10) & 0x1f);
    let mantissa = f64::from(bits & 0x3ff);
    sign * match exponent {
        0 => mantissa * 2.0_f64.powi(-24),
        0x1f if mantissa == 0.0 => f64::INFINITY,
        0x1f => f64::NAN,
        _ => (1.0 + mantissa / 1024.0) * 2.0_f64.powi(exponent - 15),
    }
}

#[test]
fn half_floats_decode_known_values() {
    let cases = [
        (0x0000, 0),
        (0x8000, 0),
        (0x3800, 32768),
        (0x3c00, MAX),
        (0x3bff, 65503),
        (0x4000, MAX),
        (0x7c00, MAX),
        (0xbc00, 0),
        (0xfc00, 0),
        (0x7e00, 0),
        (0x0001, 0),
        (0x03ff, 4),
    ];
    for (bits, expected) in cases {
        assert_eq!(half_to_u16(bits), expected, "{bits:#06x}");
    }
}

#[test]
fn every_half_float_rounds_like_the_exact_value() {
    for bits in 0..=u16::MAX {
        let value = half_value(bits);
        let expected = if value.is_nan() {
            0.0
        } else {
            (value.clamp(0.0, 1.0) * 65535.0 + 0.5).floor()
        };
        assert_eq!(
            f64::from(half_to_u16(bits)).to_bits(),
            expected.to_bits(),
            "{bits:#06x}"
        );
    }
}

#[test]
fn float_channels_clamp_to_the_unit_range() {
    assert_eq!(unit_to_u16(0.0), 0);
    assert_eq!(unit_to_u16(1.0), MAX);
    assert_eq!(unit_to_u16(0.5), 32768);
    assert_eq!(unit_to_u16(-3.0), 0);
    assert_eq!(unit_to_u16(9.5), MAX);
    assert_eq!(unit_to_u16(f32::INFINITY), MAX);
    assert_eq!(unit_to_u16(f32::NEG_INFINITY), 0);
    assert_eq!(unit_to_u16(f32::NAN), 0);
}

#[test]
fn channel_widening_maps_full_scale_to_full_scale() {
    assert_eq!(widen8(0), 0);
    assert_eq!(widen8(255), MAX);
    assert_eq!(widen8(0x1ff), MAX);
    assert_eq!(widen10(0), 0);
    assert_eq!(widen10(1023), MAX);
    assert_eq!(widen10(512), 0x8020);
}

#[test]
fn luma_uses_rec709_weights() {
    assert_eq!(WEIGHT_R + WEIGHT_G + WEIGHT_B, 1 << 16);
    assert_eq!(luma(MAX, MAX, MAX), LUMA_MAX);
    assert_eq!(luma(0, 0, 0), 0);
    let unit = f64::from(LUMA_MAX);
    for (rgb, weight) in [
        ((MAX, 0, 0), 0.2126),
        ((0, MAX, 0), 0.7152),
        ((0, 0, MAX), 0.0722),
    ] {
        let value = f64::from(luma(rgb.0, rgb.1, rgb.2)) / unit;
        assert!((value - weight).abs() < 1e-4, "{rgb:?}: {value}");
    }
}

#[test]
fn byte_orders_pick_the_right_channels() {
    let red = luma(MAX, 0, 0);
    assert_eq!(rgbx([255, 0, 0, 0]), red);
    assert_eq!(bgrx([0, 0, 255, 0]), red);
    assert_eq!(xrgb([0, 255, 0, 0]), red);
    assert_eq!(argb32(0x00ff_0000_u32.to_ne_bytes()), red);
    assert_eq!(bgr30(0x0000_03ff_u32.to_ne_bytes()), red);
    assert_eq!(rgb30(0x3ff0_0000_u32.to_ne_bytes()), red);
}

#[test]
fn the_half_table_matches_the_arithmetic_decode() {
    let table = half_table();
    for bits in 0..=u16::MAX {
        assert_eq!(u32::from(table[usize::from(bits)]), half_to_u16(bits));
    }
    let px = |r: u16, g: u16, b: u16| {
        let [r0, r1] = r.to_ne_bytes();
        let [g0, g1] = g.to_ne_bytes();
        let [b0, b1] = b.to_ne_bytes();
        [r0, r1, g0, g1, b0, b1, 0, 0]
    };
    assert_eq!(rgba16f(table, px(0x3c00, 0, 0)), luma(MAX, 0, 0));
    assert_eq!(rgba16f(table, px(0x3c00, 0x3c00, 0x3c00)), LUMA_MAX);
}
