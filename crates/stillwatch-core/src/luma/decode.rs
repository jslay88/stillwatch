//! Per-layout pixel decoders.
//!
//! Every decoder returns Rec. 709 luma on a 16-bit scale (0 to 65535),
//! computed on the encoded channel values. Channels of every depth are first
//! widened to 16 bits, so 8-bit, 10-bit, 16-bit, and float frames of the same
//! picture land on the same scale.

use std::sync::LazyLock;

/// Rec. 709 weights scaled to sum to exactly 65536.
const WEIGHT_R: u32 = 13_933;
const WEIGHT_G: u32 = 46_871;
const WEIGHT_B: u32 = 4_732;

/// The maximum luma a decoder returns.
pub(super) const LUMA_MAX: u16 = u16::MAX;
const MAX: u32 = 0xffff;

/// Every half float's 16-bit value. Baseline x86-64 has no per-lane variable
/// shift, so decoding halves arithmetically doesn't vectorize; a 128 KiB table
/// that stays in cache is several times faster.
static HALF_TABLE: LazyLock<Box<[u16; 0x1_0000]>> = LazyLock::new(|| {
    let mut table = Box::new([0; 0x1_0000]);
    for (slot, bits) in table.iter_mut().zip(0..=u16::MAX) {
        *slot = narrow(half_to_u16(bits));
    }
    table
});

/// Rec. 709 luma of 16-bit channels.
fn luma(r: u32, g: u32, b: u32) -> u16 {
    // At most 65535 * 65536 + 32768, which still fits in a u32.
    narrow((WEIGHT_R * r + WEIGHT_G * g + WEIGHT_B * b + 0x8000) >> 16)
}

fn narrow(value: u32) -> u16 {
    u16::try_from(value).unwrap_or(LUMA_MAX)
}

const fn widen8(value: u32) -> u32 {
    (value & 0xff) * 0x101
}

const fn widen10(value: u32) -> u32 {
    let value = value & 0x3ff;
    (value << 6) | (value >> 4)
}

/// Native-endian `0xAARRGGBB` words.
pub(super) fn argb32(px: [u8; 4]) -> u16 {
    let word = u32::from_ne_bytes(px);
    luma(widen8(word >> 16), widen8(word >> 8), widen8(word))
}

fn bytes(r: u8, g: u8, b: u8) -> u16 {
    luma(widen8(r.into()), widen8(g.into()), widen8(b.into()))
}

/// Bytes R, G, B, X.
pub(super) fn rgbx([r, g, b, _]: [u8; 4]) -> u16 {
    bytes(r, g, b)
}

/// Bytes B, G, R, X.
pub(super) fn bgrx([b, g, r, _]: [u8; 4]) -> u16 {
    bytes(r, g, b)
}

/// Bytes X, R, G, B.
pub(super) fn xrgb([_, r, g, b]: [u8; 4]) -> u16 {
    bytes(r, g, b)
}

/// Native-endian words with 10-bit red in the low bits.
pub(super) fn bgr30(px: [u8; 4]) -> u16 {
    let word = u32::from_ne_bytes(px);
    luma(widen10(word), widen10(word >> 10), widen10(word >> 20))
}

/// Native-endian words with 10-bit blue in the low bits.
pub(super) fn rgb30(px: [u8; 4]) -> u16 {
    let word = u32::from_ne_bytes(px);
    luma(widen10(word >> 20), widen10(word >> 10), widen10(word))
}

/// Four native-endian `u16` channels: R, G, B, A.
pub(super) fn rgba16([r0, r1, g0, g1, b0, b1, _, _]: [u8; 8]) -> u16 {
    let channel = |bytes| u32::from(u16::from_ne_bytes(bytes));
    luma(channel([r0, r1]), channel([g0, g1]), channel([b0, b1]))
}

/// The half-float lookup table for [`rgba16f`], built on first use.
pub(super) fn half_table() -> &'static [u16; 0x1_0000] {
    &HALF_TABLE
}

/// Four native-endian half-float channels: R, G, B, A, looked up in
/// [`half_table`].
pub(super) fn rgba16f(table: &[u16; 0x1_0000], [r0, r1, g0, g1, b0, b1, _, _]: [u8; 8]) -> u16 {
    let channel = |bytes| u32::from(table[usize::from(u16::from_ne_bytes(bytes))]);
    luma(channel([r0, r1]), channel([g0, g1]), channel([b0, b1]))
}

/// Four native-endian `f32` channels: R, G, B, A.
pub(super) fn rgba32f(px: [u8; 16]) -> u16 {
    let [r0, r1, r2, r3, g0, g1, g2, g3, b0, b1, b2, b3, ..] = px;
    let channel = |bytes| unit_to_u16(f32::from_ne_bytes(bytes));
    luma(
        channel([r0, r1, r2, r3]),
        channel([g0, g1, g2, g3]),
        channel([b0, b1, b2, b3]),
    )
}

/// Maps a float channel to 16 bits, clamping to [0, 1] first so HDR values
/// above 1.0 read as white. NaN reads as black.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is clamped to [0.5, 65535.5] before the cast"
)]
fn unit_to_u16(value: f32) -> u32 {
    if value.is_nan() {
        return 0;
    }
    (value.clamp(0.0, 1.0) * 65535.0 + 0.5) as u32
}

/// Maps IEEE 754 binary16 bits to 16 bits with the same clamping as
/// [`unit_to_u16`], rounding to nearest.
fn half_to_u16(bits: u16) -> u32 {
    let bits = u32::from(bits);
    let exponent = (bits >> 10) & 0x1f;
    let mantissa = bits & 0x3ff;
    // Subnormals scale like exponent 1 without the implicit leading bit.
    let significand = if exponent == 0 {
        mantissa
    } else {
        mantissa | 0x400
    };
    let shift = 25 - exponent.clamp(1, 14);
    let below_one = (significand * MAX + (1 << (shift - 1))) >> shift;
    // Positive halves order like their bits: 0x3c00 is 1.0, 0x7c00 is
    // infinity, above that are NaNs and then every negative value.
    if bits > 0x7c00 {
        0
    } else if bits >= 0x3c00 {
        MAX
    } else {
        below_one
    }
}

#[cfg(test)]
mod tests;
