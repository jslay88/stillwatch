//! Raw frame to luma grid, in one pass over the frame.

use super::area::{split, sum_boxes};
use super::decode::{self, LUMA_MAX};
use super::format::Layout;
use super::{FrameError, LumaGrid, RawFrame};

/// Converts `frame` to Rec. 709 luma and downscales it to `downscale_width`
/// cells wide, preserving the aspect ratio.
///
/// Every source pixel contributes to exactly one cell (area averaging), so
/// thin static content such as a one-pixel line still moves its cell's mean.
/// Frames narrower than `downscale_width` keep their own width; nothing is
/// upscaled. The frame is read once, row by row, and no full-resolution
/// luma buffer is allocated.
///
/// # Errors
///
/// Returns [`FrameError`] when the frame is empty, its stride or buffer is
/// too small for its dimensions, or `downscale_width` is zero.
pub fn downscale(frame: &RawFrame<'_>, downscale_width: u32) -> Result<LumaGrid, FrameError> {
    if downscale_width == 0 {
        return Err(FrameError::ZeroDownscaleWidth);
    }
    let row_bytes = frame.validate()?;
    let width = downscale_width.min(frame.width);
    let height = scaled_height(frame.width, frame.height, width);
    let pass = Pass {
        frame,
        row_bytes,
        widths: split(frame.width, width),
        heights: split(frame.height, height),
    };
    let data = match frame.format.layout() {
        Layout::Argb32Word => pass.run(decode::argb32),
        Layout::BytesRgbx => pass.run(decode::rgbx),
        Layout::BytesBgrx => pass.run(decode::bgrx),
        Layout::BytesXrgb => pass.run(decode::xrgb),
        Layout::Bgr30Word => pass.run(decode::bgr30),
        Layout::Rgb30Word => pass.run(decode::rgb30),
        Layout::Rgba16 => pass.run(decode::rgba16),
        Layout::Rgba16F => {
            let table = decode::half_table();
            pass.run(|px| decode::rgba16f(table, px))
        }
        Layout::Rgba32F => pass.run(decode::rgba32f),
    };
    Ok(LumaGrid::new(width, height, data)?)
}

/// `source_height * width / source_width`, rounded, and at least one row.
fn scaled_height(source_width: u32, source_height: u32, width: u32) -> u32 {
    let scaled = (u64::from(source_height) * u64::from(width) + u64::from(source_width / 2))
        / u64::from(source_width.max(1));
    u32::try_from(scaled)
        .unwrap_or(source_height)
        .clamp(1, source_height.max(1))
}

struct Pass<'f, 'a> {
    frame: &'f RawFrame<'a>,
    row_bytes: usize,
    widths: Vec<usize>,
    heights: Vec<usize>,
}

impl Pass<'_, '_> {
    fn run<const N: usize>(&self, luma: impl Fn([u8; N]) -> u16) -> Vec<u8> {
        let rows = (0..self.frame.height as usize)
            .map_while(|y| self.frame.row(y, self.row_bytes))
            .map(|row| row.as_chunks::<N>().0);
        let mut data = Vec::with_capacity(self.widths.len() * self.heights.len());
        sum_boxes(
            rows,
            &self.widths,
            &self.heights,
            |&px| luma(px),
            |sum, count| data.push(to_u8(sum, count)),
        );
        data
    }
}

/// The mean of `count` 16-bit luma values summing to `sum`, as 8-bit luma.
fn to_u8(sum: u64, count: usize) -> u8 {
    let count = u64::try_from(count).unwrap_or(u64::MAX).max(1);
    let mean = sum.saturating_add(count / 2) / count;
    let scaled =
        (mean.min(u64::from(LUMA_MAX)) * 255 + u64::from(LUMA_MAX / 2)) / u64::from(LUMA_MAX);
    u8::try_from(scaled).unwrap_or(u8::MAX)
}

#[cfg(test)]
mod tests;
