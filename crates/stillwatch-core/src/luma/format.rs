//! Pixel formats that capture backends hand to [`downscale`](super::downscale).

use std::fmt;

/// The memory layout of a captured frame.
///
/// Most variants are named after the `QImage::Format` that `KWin` `ScreenShot2`
/// reports. The `*8888` variants are byte-ordered (first byte first in
/// memory) and also cover the `PipeWire` / SPA video formats of the same name.
/// The `*32` and `*30` variants are native-endian 32-bit words, as in Qt.
///
/// Alpha is ignored when computing luma. Captured frames are opaque, and a
/// premultiplied pixel is already the color composited over black.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PixelFormat {
    /// `QImage::Format_RGB32`: native-endian `0xffRRGGBB` words.
    Rgb32,
    /// `QImage::Format_ARGB32`: native-endian `0xAARRGGBB` words.
    Argb32,
    /// `QImage::Format_ARGB32_Premultiplied`: native-endian `0xAARRGGBB` words.
    Argb32Premultiplied,
    /// `QImage::Format_RGBX8888`, SPA `RGBx`: bytes R, G, B, X.
    Rgbx8888,
    /// `QImage::Format_RGBA8888`, SPA `RGBA`: bytes R, G, B, A.
    Rgba8888,
    /// `QImage::Format_RGBA8888_Premultiplied`: bytes R, G, B, A.
    Rgba8888Premultiplied,
    /// SPA `BGRx`: bytes B, G, R, X.
    Bgrx8888,
    /// SPA `BGRA`: bytes B, G, R, A.
    Bgra8888,
    /// SPA `xRGB`: bytes X, R, G, B.
    Xrgb8888,
    /// SPA `ARGB`: bytes A, R, G, B.
    Argb8888,
    /// `QImage::Format_BGR30`: native-endian words, 2-bit X and 10-bit B, G, R
    /// from the top down (red in the low bits).
    Bgr30,
    /// `QImage::Format_A2BGR30_Premultiplied`: like [`Self::Bgr30`] with a
    /// 2-bit alpha.
    A2Bgr30Premultiplied,
    /// `QImage::Format_RGB30`: native-endian words, 2-bit X and 10-bit R, G, B
    /// from the top down (blue in the low bits).
    Rgb30,
    /// `QImage::Format_A2RGB30_Premultiplied`: like [`Self::Rgb30`] with a
    /// 2-bit alpha.
    A2Rgb30Premultiplied,
    /// `QImage::Format_RGBX64`: native-endian `u16` R, G, B, X.
    Rgbx64,
    /// `QImage::Format_RGBA64`: native-endian `u16` R, G, B, A.
    Rgba64,
    /// `QImage::Format_RGBA64_Premultiplied`: native-endian `u16` R, G, B, A.
    Rgba64Premultiplied,
    /// `QImage::Format_RGBX16FPx4`: native-endian half floats R, G, B, X.
    Rgbx16Fpx4,
    /// `QImage::Format_RGBA16FPx4`: native-endian half floats R, G, B, A.
    Rgba16Fpx4,
    /// `QImage::Format_RGBA16FPx4_Premultiplied`: native-endian half floats.
    Rgba16Fpx4Premultiplied,
    /// `QImage::Format_RGBX32FPx4`: native-endian `f32` R, G, B, X.
    Rgbx32Fpx4,
    /// `QImage::Format_RGBA32FPx4`: native-endian `f32` R, G, B, A.
    Rgba32Fpx4,
    /// `QImage::Format_RGBA32FPx4_Premultiplied`: native-endian `f32`.
    Rgba32Fpx4Premultiplied,
}

/// How a [`PixelFormat`] is decoded. Formats that differ only in alpha
/// handling share a layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Layout {
    /// Native-endian `0xAARRGGBB` word.
    Argb32Word,
    /// Bytes R, G, B, X.
    BytesRgbx,
    /// Bytes B, G, R, X.
    BytesBgrx,
    /// Bytes X, R, G, B.
    BytesXrgb,
    /// Native-endian word with 10-bit red in the low bits.
    Bgr30Word,
    /// Native-endian word with 10-bit blue in the low bits.
    Rgb30Word,
    /// Four native-endian `u16`.
    Rgba16,
    /// Four native-endian half floats.
    Rgba16F,
    /// Four native-endian `f32`.
    Rgba32F,
}

impl Layout {
    /// Bytes per pixel.
    pub(super) const fn bytes_per_pixel(self) -> usize {
        match self {
            Self::Argb32Word
            | Self::BytesRgbx
            | Self::BytesBgrx
            | Self::BytesXrgb
            | Self::Bgr30Word
            | Self::Rgb30Word => 4,
            Self::Rgba16 | Self::Rgba16F => 8,
            Self::Rgba32F => 16,
        }
    }
}

impl PixelFormat {
    /// Every supported format.
    pub const ALL: [Self; 23] = [
        Self::Rgb32,
        Self::Argb32,
        Self::Argb32Premultiplied,
        Self::Rgbx8888,
        Self::Rgba8888,
        Self::Rgba8888Premultiplied,
        Self::Bgrx8888,
        Self::Bgra8888,
        Self::Xrgb8888,
        Self::Argb8888,
        Self::Bgr30,
        Self::A2Bgr30Premultiplied,
        Self::Rgb30,
        Self::A2Rgb30Premultiplied,
        Self::Rgbx64,
        Self::Rgba64,
        Self::Rgba64Premultiplied,
        Self::Rgbx16Fpx4,
        Self::Rgba16Fpx4,
        Self::Rgba16Fpx4Premultiplied,
        Self::Rgbx32Fpx4,
        Self::Rgba32Fpx4,
        Self::Rgba32Fpx4Premultiplied,
    ];

    /// The format for a `QImage::Format` value, as `KWin` `ScreenShot2` reports
    /// it in its `format` result.
    ///
    /// # Errors
    ///
    /// Returns [`UnsupportedFormat`] for formats Stillwatch can't decode
    /// (indexed, 16-bit packed, 24-bit, grayscale, CMYK, ...).
    pub fn from_qimage(code: u32) -> Result<Self, UnsupportedFormat> {
        Self::ALL
            .into_iter()
            .find(|format| format.qimage_code() == Some(code))
            .ok_or(UnsupportedFormat {
                family: FormatFamily::QImage,
                code,
            })
    }

    /// The format for an `spa_video_format` value, as `PipeWire` reports it.
    ///
    /// Codes are the stable `spa_video_format` discriminants (`RGBx` is 7,
    /// `BGRx` is 8, and so on). Packed 24-bit, planar YUV, and the 10-bit
    /// `*_210LE` layouts are rejected.
    ///
    /// # Errors
    ///
    /// Returns [`UnsupportedFormat`] for formats Stillwatch can't decode.
    pub fn from_spa(code: u32) -> Result<Self, UnsupportedFormat> {
        let format = match code {
            7 => Self::Rgbx8888,
            8 => Self::Bgrx8888,
            9 => Self::Xrgb8888,
            11 => Self::Rgba8888,
            12 => Self::Bgra8888,
            13 => Self::Argb8888,
            78 => Self::Rgba16Fpx4,
            79 => Self::Rgba32Fpx4,
            _ => {
                return Err(UnsupportedFormat {
                    family: FormatFamily::Spa,
                    code,
                });
            }
        };
        Ok(format)
    }

    /// The `QImage::Format` value, or `None` for the SPA-only byte orders.
    #[must_use]
    pub const fn qimage_code(self) -> Option<u32> {
        let code = match self {
            Self::Rgb32 => 4,
            Self::Argb32 => 5,
            Self::Argb32Premultiplied => 6,
            Self::Rgbx8888 => 16,
            Self::Rgba8888 => 17,
            Self::Rgba8888Premultiplied => 18,
            Self::Bgr30 => 19,
            Self::A2Bgr30Premultiplied => 20,
            Self::Rgb30 => 21,
            Self::A2Rgb30Premultiplied => 22,
            Self::Rgbx64 => 25,
            Self::Rgba64 => 26,
            Self::Rgba64Premultiplied => 27,
            Self::Rgbx16Fpx4 => 30,
            Self::Rgba16Fpx4 => 31,
            Self::Rgba16Fpx4Premultiplied => 32,
            Self::Rgbx32Fpx4 => 33,
            Self::Rgba32Fpx4 => 34,
            Self::Rgba32Fpx4Premultiplied => 35,
            Self::Bgrx8888 | Self::Bgra8888 | Self::Xrgb8888 | Self::Argb8888 => return None,
        };
        Some(code)
    }

    /// Bytes per pixel.
    #[must_use]
    pub const fn bytes_per_pixel(self) -> usize {
        self.layout().bytes_per_pixel()
    }

    pub(super) const fn layout(self) -> Layout {
        match self {
            Self::Rgb32 | Self::Argb32 | Self::Argb32Premultiplied => Layout::Argb32Word,
            Self::Rgbx8888 | Self::Rgba8888 | Self::Rgba8888Premultiplied => Layout::BytesRgbx,
            Self::Bgrx8888 | Self::Bgra8888 => Layout::BytesBgrx,
            Self::Xrgb8888 | Self::Argb8888 => Layout::BytesXrgb,
            Self::Bgr30 | Self::A2Bgr30Premultiplied => Layout::Bgr30Word,
            Self::Rgb30 | Self::A2Rgb30Premultiplied => Layout::Rgb30Word,
            Self::Rgbx64 | Self::Rgba64 | Self::Rgba64Premultiplied => Layout::Rgba16,
            Self::Rgbx16Fpx4 | Self::Rgba16Fpx4 | Self::Rgba16Fpx4Premultiplied => Layout::Rgba16F,
            Self::Rgbx32Fpx4 | Self::Rgba32Fpx4 | Self::Rgba32Fpx4Premultiplied => Layout::Rgba32F,
        }
    }
}

/// Where an unsupported format code came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FormatFamily {
    /// A `QImage::Format` value (`KWin` `ScreenShot2`).
    QImage,
    /// An `spa_video_format` value (`PipeWire`).
    Spa,
}

impl fmt::Display for FormatFamily {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::QImage => "QImage",
            Self::Spa => "SPA video",
        })
    }
}

/// A capture backend reported a pixel format Stillwatch can't decode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
#[error("unsupported {family} format {code}")]
pub struct UnsupportedFormat {
    /// Which enumeration `code` belongs to.
    pub family: FormatFamily,
    /// The raw format value.
    pub code: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qimage_codes_round_trip() {
        for format in PixelFormat::ALL {
            if let Some(code) = format.qimage_code() {
                assert_eq!(PixelFormat::from_qimage(code), Ok(format));
            }
        }
        assert_eq!(
            PixelFormat::from_qimage(6),
            Ok(PixelFormat::Argb32Premultiplied)
        );
        assert_eq!(PixelFormat::from_qimage(31), Ok(PixelFormat::Rgba16Fpx4));
    }

    #[test]
    fn spa_codes_cover_the_byte_orders_pipewire_sends() {
        assert_eq!(PixelFormat::from_spa(7), Ok(PixelFormat::Rgbx8888));
        assert_eq!(PixelFormat::from_spa(8), Ok(PixelFormat::Bgrx8888));
        assert_eq!(PixelFormat::from_spa(9), Ok(PixelFormat::Xrgb8888));
        assert_eq!(PixelFormat::from_spa(11), Ok(PixelFormat::Rgba8888));
        assert_eq!(PixelFormat::from_spa(12), Ok(PixelFormat::Bgra8888));
        assert_eq!(PixelFormat::from_spa(13), Ok(PixelFormat::Argb8888));
        assert_eq!(PixelFormat::from_spa(78), Ok(PixelFormat::Rgba16Fpx4));
        assert_eq!(PixelFormat::from_spa(79), Ok(PixelFormat::Rgba32Fpx4));
        for code in [0, 1, 10, 14, 15, 16, 80] {
            let err = PixelFormat::from_spa(code).unwrap_err();
            assert_eq!(err.family, FormatFamily::Spa);
            assert_eq!(err.code, code);
        }
    }

    #[test]
    fn unknown_qimage_codes_error() {
        for code in [0, 3, 7, 13, 24, 28, 29, 36, 1000] {
            let err = PixelFormat::from_qimage(code).unwrap_err();
            assert_eq!(err.code, code);
            assert_eq!(err.to_string(), format!("unsupported QImage format {code}"));
        }
        let spa = UnsupportedFormat {
            family: FormatFamily::Spa,
            code: 9,
        };
        assert_eq!(spa.to_string(), "unsupported SPA video format 9");
    }

    #[test]
    fn bytes_per_pixel_follows_the_layout() {
        assert_eq!(PixelFormat::Argb32Premultiplied.bytes_per_pixel(), 4);
        assert_eq!(PixelFormat::Xrgb8888.bytes_per_pixel(), 4);
        assert_eq!(PixelFormat::A2Bgr30Premultiplied.bytes_per_pixel(), 4);
        assert_eq!(PixelFormat::Rgba64.bytes_per_pixel(), 8);
        assert_eq!(PixelFormat::Rgba16Fpx4.bytes_per_pixel(), 8);
        assert_eq!(PixelFormat::Rgbx32Fpx4.bytes_per_pixel(), 16);
    }
}
