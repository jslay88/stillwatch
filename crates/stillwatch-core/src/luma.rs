//! Downscaled luma images and output descriptions.
//!
//! A [`LumaGrid`] is the only form a captured frame takes after a capture
//! backend is done with it. It never leaves the daemon: it is not serializable,
//! and only per-block states and percentages derived from it are ever sent.

use serde::{Deserialize, Serialize};

/// A connected output (monitor) as reported by a capture backend.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OutputInfo {
    /// Connector name, for example `HDMI-A-1`.
    pub name: String,
    /// Width in physical pixels.
    pub width: u32,
    /// Height in physical pixels.
    pub height: u32,
}

impl OutputInfo {
    /// Creates an output description.
    #[must_use]
    pub fn new(name: impl Into<String>, width: u32, height: u32) -> Self {
        Self {
            name: name.into(),
            width,
            height,
        }
    }
}

/// Error returned when building a [`LumaGrid`] from inconsistent parts.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LumaGridError {
    /// The buffer length doesn't equal `width * height`.
    #[error("luma buffer has {actual} bytes, expected {expected}")]
    SizeMismatch {
        /// `width * height`.
        expected: usize,
        /// The length of the buffer that was passed.
        actual: usize,
    },
    /// `width * height` doesn't fit in memory.
    #[error("luma grid dimensions {width}x{height} overflow")]
    TooLarge {
        /// Requested width.
        width: u32,
        /// Requested height.
        height: u32,
    },
}

/// A row-major grid of luma values (0 = black, 255 = white).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LumaGrid {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl LumaGrid {
    /// Wraps an existing row-major buffer of `width * height` luma values.
    ///
    /// # Errors
    ///
    /// Returns [`LumaGridError::SizeMismatch`] if `data` has the wrong length,
    /// or [`LumaGridError::TooLarge`] if the dimensions overflow `usize`.
    pub fn new(width: u32, height: u32, data: Vec<u8>) -> Result<Self, LumaGridError> {
        let expected = Self::area(width, height)?;
        if data.len() != expected {
            return Err(LumaGridError::SizeMismatch {
                expected,
                actual: data.len(),
            });
        }
        Ok(Self {
            width,
            height,
            data,
        })
    }

    /// A grid where every cell has the same luma.
    ///
    /// # Errors
    ///
    /// Returns [`LumaGridError::TooLarge`] if the dimensions overflow `usize`.
    pub fn filled(width: u32, height: u32, value: u8) -> Result<Self, LumaGridError> {
        let len = Self::area(width, height)?;
        Ok(Self {
            width,
            height,
            data: vec![value; len],
        })
    }

    /// A grid whose cell at `(x, y)` is `f(x, y)`.
    ///
    /// # Errors
    ///
    /// Returns [`LumaGridError::TooLarge`] if the dimensions overflow `usize`.
    pub fn from_fn(
        width: u32,
        height: u32,
        mut f: impl FnMut(u32, u32) -> u8,
    ) -> Result<Self, LumaGridError> {
        let len = Self::area(width, height)?;
        let mut data = Vec::with_capacity(len);
        for y in 0..height {
            for x in 0..width {
                data.push(f(x, y));
            }
        }
        Ok(Self {
            width,
            height,
            data,
        })
    }

    /// Width in cells.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Height in cells.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// The row-major luma buffer.
    #[must_use]
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Consumes the grid and returns its buffer.
    #[must_use]
    pub fn into_data(self) -> Vec<u8> {
        self.data
    }

    /// The luma at `(x, y)`, or `None` when out of bounds.
    #[must_use]
    pub fn get(&self, x: u32, y: u32) -> Option<u8> {
        if x >= self.width {
            return None;
        }
        self.row(y)?.get(x as usize).copied()
    }

    /// Row `y`, or `None` when out of bounds.
    #[must_use]
    pub fn row(&self, y: u32) -> Option<&[u8]> {
        self.data
            .chunks_exact((self.width as usize).max(1))
            .nth(y as usize)
            .filter(|_| self.width > 0)
    }

    fn area(width: u32, height: u32) -> Result<usize, LumaGridError> {
        (width as usize)
            .checked_mul(height as usize)
            .ok_or(LumaGridError::TooLarge { width, height })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_checks_buffer_length() {
        let grid = LumaGrid::new(2, 2, vec![1, 2, 3, 4]).unwrap();
        assert_eq!((grid.width(), grid.height()), (2, 2));
        assert_eq!(grid.data(), &[1, 2, 3, 4]);
        assert_eq!(
            LumaGrid::new(2, 2, vec![0; 3]),
            Err(LumaGridError::SizeMismatch {
                expected: 4,
                actual: 3
            })
        );
    }

    #[test]
    fn filled_and_from_fn_build_row_major_grids() {
        assert_eq!(LumaGrid::filled(3, 2, 9).unwrap().into_data(), vec![9; 6]);
        let grid = LumaGrid::from_fn(3, 2, |x, y| u8::try_from(y * 10 + x).unwrap()).unwrap();
        assert_eq!(grid.data(), &[0, 1, 2, 10, 11, 12]);
        assert_eq!(grid.row(1), Some(&[10, 11, 12][..]));
        assert_eq!(grid.get(2, 1), Some(12));
    }

    #[test]
    fn out_of_bounds_access_is_none() {
        let grid = LumaGrid::filled(2, 2, 0).unwrap();
        assert_eq!(grid.get(2, 0), None);
        assert_eq!(grid.get(0, 2), None);
        assert_eq!(grid.row(2), None);
        let empty = LumaGrid::new(0, 3, Vec::new()).unwrap();
        assert_eq!(empty.row(0), None);
        assert_eq!(empty.get(0, 0), None);
    }

    #[test]
    fn errors_have_readable_messages() {
        let err = LumaGrid::new(1, 1, vec![]).unwrap_err();
        assert_eq!(err.to_string(), "luma buffer has 0 bytes, expected 1");
        let err = LumaGridError::TooLarge {
            width: 1,
            height: 2,
        };
        assert_eq!(err.to_string(), "luma grid dimensions 1x2 overflow");
    }

    #[test]
    fn output_info_serializes_as_plain_fields() {
        let output = OutputInfo::new("HDMI-A-1", 3840, 2160);
        let json = serde_json::to_string(&output).unwrap();
        assert_eq!(json, r#"{"name":"HDMI-A-1","width":3840,"height":2160}"#);
        assert_eq!(serde_json::from_str::<OutputInfo>(&json).unwrap(), output);
    }
}
