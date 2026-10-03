//! Borrowed raw frames and their validation.

use std::fmt;

use super::{LumaGridError, PixelFormat, UnsupportedFormat};
use crate::backend::BackendError;

/// A captured frame, borrowed from the capture backend's buffer.
///
/// Rows are `stride` bytes apart, and the last row may stop right after its
/// last pixel. `Debug` prints the buffer length, never its contents.
#[derive(Clone, Copy)]
pub struct RawFrame<'a> {
    /// The pixel layout of `data`.
    pub format: PixelFormat,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Bytes from the start of one row to the start of the next.
    pub stride: usize,
    /// The pixel buffer.
    pub data: &'a [u8],
}

impl fmt::Debug for RawFrame<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RawFrame")
            .field("format", &self.format)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("stride", &self.stride)
            .field("data_len", &self.data.len())
            .finish()
    }
}

impl<'a> RawFrame<'a> {
    /// Checks that the dimensions, stride, and buffer agree, and returns the
    /// number of pixel bytes in each row.
    pub(super) fn validate(&self) -> Result<usize, FrameError> {
        let too_large = || FrameError::TooLarge {
            width: self.width,
            height: self.height,
        };
        if self.width == 0 || self.height == 0 {
            return Err(FrameError::Empty {
                width: self.width,
                height: self.height,
            });
        }
        let row_bytes = (self.width as usize)
            .checked_mul(self.format.bytes_per_pixel())
            .ok_or_else(too_large)?;
        if self.stride < row_bytes {
            return Err(FrameError::StrideTooSmall {
                stride: self.stride,
                row_bytes,
            });
        }
        let expected = self
            .stride
            .checked_mul(self.height as usize - 1)
            .and_then(|start| start.checked_add(row_bytes))
            .ok_or_else(too_large)?;
        if self.data.len() < expected {
            return Err(FrameError::BufferTooShort {
                expected,
                actual: self.data.len(),
            });
        }
        Ok(row_bytes)
    }

    /// The pixel bytes of row `y`, without stride padding.
    pub(super) fn row(&self, y: usize, row_bytes: usize) -> Option<&'a [u8]> {
        let start = y.checked_mul(self.stride)?;
        self.data.get(start..start.checked_add(row_bytes)?)
    }
}

/// Why a frame couldn't be converted to a luma grid.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FrameError {
    /// The frame has no pixels.
    #[error("frame is empty ({width}x{height})")]
    Empty {
        /// Frame width.
        width: u32,
        /// Frame height.
        height: u32,
    },
    /// The requested grid width is zero.
    #[error("downscale width must be at least 1")]
    ZeroDownscaleWidth,
    /// A row's pixels don't fit in the stride.
    #[error("stride of {stride} bytes is shorter than a {row_bytes}-byte row")]
    StrideTooSmall {
        /// The stride that was passed.
        stride: usize,
        /// `width * bytes_per_pixel`.
        row_bytes: usize,
    },
    /// The buffer ends before the last row does.
    #[error("frame buffer has {actual} bytes, expected at least {expected}")]
    BufferTooShort {
        /// `stride * (height - 1) + width * bytes_per_pixel`.
        expected: usize,
        /// The length of the buffer that was passed.
        actual: usize,
    },
    /// The frame's byte size doesn't fit in memory.
    #[error("frame dimensions {width}x{height} overflow")]
    TooLarge {
        /// Frame width.
        width: u32,
        /// Frame height.
        height: u32,
    },
    /// The output grid couldn't be built.
    #[error(transparent)]
    Grid(#[from] LumaGridError),
}

impl From<FrameError> for BackendError {
    fn from(err: FrameError) -> Self {
        Self::Protocol(err.to_string())
    }
}

impl From<UnsupportedFormat> for BackendError {
    fn from(err: UnsupportedFormat) -> Self {
        Self::Unsupported(err.to_string())
    }
}
