//! The metadata half of a `CaptureScreen` reply.
//!
//! `KWin` 6.7.5 replies with `type` (`"raw"`), `format` (a `QImage::Format`),
//! `width`, `height`, `stride` (all `u32`), `scale` (`f64`, the output's
//! device pixel ratio), and `screen` (the connector name), then writes
//! `stride * height` bytes into the pipe.

use std::collections::HashMap;

use stillwatch_core::backend::BackendError;
use stillwatch_core::luma::{PixelFormat, RawFrame};
use zbus::zvariant::OwnedValue;

/// The largest frame Stillwatch will allocate for, so a corrupt reply can't
/// ask for an absurd buffer. An 8K frame of four `f32` channels is 531 MB.
pub const MAX_FRAME_BYTES: usize = 1 << 30;

/// What `KWin` said about the image it's about to write. Holds no pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameMeta {
    /// The pixel layout.
    pub format: PixelFormat,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Bytes from one row to the next.
    pub stride: u32,
    /// The output's scale factor, when `KWin` reports it.
    pub scale: Option<f64>,
    /// The connector `KWin` captured, when it reports it.
    pub screen: Option<String>,
}

impl FrameMeta {
    /// Parses a `CaptureScreen` result map.
    ///
    /// # Errors
    ///
    /// [`BackendError::Protocol`] for a missing or mistyped key,
    /// [`BackendError::Unsupported`] for a `type` other than `raw` or a
    /// format Stillwatch can't decode.
    pub fn parse(results: &HashMap<String, OwnedValue>) -> Result<Self, BackendError> {
        if let Some(kind) = optional::<&str>(results, "type")?
            && kind != "raw"
        {
            return Err(BackendError::Unsupported(format!(
                "ScreenShot2 image type {kind:?}"
            )));
        }
        Ok(Self {
            format: PixelFormat::from_qimage(required(results, "format")?)?,
            width: required(results, "width")?,
            height: required(results, "height")?,
            stride: required(results, "stride")?,
            scale: optional(results, "scale")?,
            screen: optional::<&str>(results, "screen")?.map(str::to_owned),
        })
    }

    /// How many bytes `KWin` writes into the pipe: `stride * height`.
    ///
    /// # Errors
    ///
    /// [`BackendError::Protocol`] when that exceeds [`MAX_FRAME_BYTES`].
    pub fn byte_len(&self) -> Result<usize, BackendError> {
        (self.stride as usize)
            .checked_mul(self.height as usize)
            .filter(|&len| len <= MAX_FRAME_BYTES)
            .ok_or_else(|| {
                BackendError::Protocol(format!(
                    "ScreenShot2 frame of {}x{} with stride {} is larger than {MAX_FRAME_BYTES} bytes",
                    self.width, self.height, self.stride
                ))
            })
    }

    /// Wraps the bytes read from the pipe for [`downscale`](stillwatch_core::luma::downscale).
    #[must_use]
    pub fn frame<'a>(&self, data: &'a [u8]) -> RawFrame<'a> {
        RawFrame {
            format: self.format,
            width: self.width,
            height: self.height,
            stride: self.stride as usize,
            data,
        }
    }
}

fn required<'a, T>(results: &'a HashMap<String, OwnedValue>, key: &str) -> Result<T, BackendError>
where
    T: TryFrom<&'a zbus::zvariant::Value<'a>, Error = zbus::zvariant::Error>,
{
    optional(results, key)?
        .ok_or_else(|| BackendError::Protocol(format!("ScreenShot2 reply has no {key:?}")))
}

fn optional<'a, T>(
    results: &'a HashMap<String, OwnedValue>,
    key: &str,
) -> Result<Option<T>, BackendError>
where
    T: TryFrom<&'a zbus::zvariant::Value<'a>, Error = zbus::zvariant::Error>,
{
    results
        .get(key)
        .map(|value| {
            value.downcast_ref::<T>().map_err(|err| {
                BackendError::Protocol(format!(
                    "ScreenShot2 reply {key:?} has the wrong type ({}): {err}",
                    value.value_signature()
                ))
            })
        })
        .transpose()
}

#[cfg(test)]
mod tests;
