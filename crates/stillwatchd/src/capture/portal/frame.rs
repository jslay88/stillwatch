//! One mapped `PipeWire` buffer, downscaled and dropped.
//!
//! The bytes live only for the call. What remains is a [`LumaGrid`].

use stillwatch_core::backend::BackendError;
use stillwatch_core::luma::{self, LumaGrid, PixelFormat, RawFrame};

/// A single plane of a `PipeWire` buffer.
pub struct Plane<'a> {
    /// `spa_video_format` discriminant.
    pub spa_format: u32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Bytes from the start of one row to the next. Zero means packed.
    pub stride: usize,
    /// The mapped pixels. Not retained.
    pub data: &'a [u8],
}

/// Downscales `plane` to `downscale_width` cells wide.
///
/// # Errors
///
/// [`BackendError::Unsupported`] for a pixel format Stillwatch can't decode,
/// and [`BackendError::Protocol`] when the buffer doesn't match its geometry.
pub fn grid_from_plane(plane: &Plane<'_>, downscale_width: u32) -> Result<LumaGrid, BackendError> {
    let format = PixelFormat::from_spa(plane.spa_format)?;
    let stride = if plane.stride == 0 {
        plane
            .width
            .checked_mul(u32::try_from(format.bytes_per_pixel()).unwrap_or(u32::MAX))
            .map(|bytes| usize::try_from(bytes).unwrap_or(usize::MAX))
            .ok_or_else(|| BackendError::Protocol("frame stride overflows".into()))?
    } else {
        plane.stride
    };
    let frame = RawFrame {
        format,
        width: plane.width,
        height: plane.height,
        stride,
        data: plane.data,
    };
    Ok(luma::downscale(&frame, downscale_width)?)
}

/// Copies the live bytes of a mapped buffer. The caller drops the copy after
/// [`grid_from_plane`].
///
/// # Errors
///
/// [`BackendError::Unsupported`] when `PipeWire` didn't map the buffer (a
/// `DMA-BUF` `PipeWire` couldn't map, or a memfd with no pointer). Stillwatch
/// doesn't map file descriptors itself.
/// [`BackendError::Protocol`] when the chunk doesn't fit in the mapping.
pub fn copy_mapped(
    kind: pipewire::spa::buffer::DataType,
    mapped: Option<&[u8]>,
    offset: u32,
    len: u32,
) -> Result<Vec<u8>, BackendError> {
    let Some(mapped) = mapped else {
        return Err(BackendError::Unsupported(format!(
            "PipeWire didn't map the {kind:?} buffer"
        )));
    };
    let start = usize::try_from(offset).unwrap_or(usize::MAX);
    let end = start
        .checked_add(usize::try_from(len).unwrap_or(usize::MAX))
        .ok_or_else(|| BackendError::Protocol("frame chunk overflows".into()))?;
    mapped.get(start..end).map(<[u8]>::to_vec).ok_or_else(|| {
        BackendError::Protocol(format!(
            "frame chunk {start}..{end} doesn't fit in {} mapped bytes",
            mapped.len()
        ))
    })
}

#[cfg(test)]
mod tests;
