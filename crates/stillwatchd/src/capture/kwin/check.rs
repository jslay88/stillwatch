//! One-off captures that report metadata only: `stillwatchd --capture-check`
//! and the startup authorization check.

use std::fmt;
use std::io::Write as _;
use std::time::{Duration, Instant};

use anyhow::Context as _;
use stillwatch_core::backend::{BackendError, ScreenCapture as _};
use stillwatch_core::config::Config;

use super::{FrameMeta, KwinCapture};

/// What one capture looked like. Dimensions and timing, never pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct CaptureReport {
    /// The output that was captured.
    pub output: String,
    /// The frame `KWin` returned.
    pub meta: FrameMeta,
    /// Width of the luma grid it was downscaled to.
    pub grid_width: u32,
    /// Height of the luma grid.
    pub grid_height: u32,
    /// Capture, read, and downscale together.
    pub elapsed: Duration,
}

impl fmt::Display for CaptureReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let meta = &self.meta;
        write!(
            f,
            "{}: {}x{} {:?}, stride {}",
            self.output, meta.width, meta.height, meta.format, meta.stride
        )?;
        if let Some(scale) = meta.scale {
            write!(f, ", scale {scale}")?;
        }
        write!(
            f,
            " -> {}x{} luma grid in {} ms",
            self.grid_width,
            self.grid_height,
            self.elapsed.as_millis()
        )
    }
}

/// Captures `output` once and reports on it.
///
/// # Errors
///
/// Whatever [`KwinCapture::capture`] returns.
pub async fn check_output(
    capture: &KwinCapture,
    output: &str,
    downscale_width: u32,
) -> Result<CaptureReport, BackendError> {
    let started = Instant::now();
    let result = capture.capture(output, downscale_width).await?;
    Ok(CaptureReport {
        output: output.to_owned(),
        meta: result.meta,
        grid_width: result.grid.width(),
        grid_height: result.grid.height(),
        elapsed: started.elapsed(),
    })
}

/// `stillwatchd --capture-check <OUTPUT>`: captures once at the default
/// `stale.downscale_width` and prints the report to stdout.
///
/// # Errors
///
/// When `KWin` can't be reached, refuses the capture, or stdout fails.
pub async fn run_check(output: &str) -> anyhow::Result<()> {
    let capture = KwinCapture::connect().await?;
    let width = Config::default().stale.downscale_width;
    let report = check_output(&capture, output, width)
        .await
        .with_context(|| format!("capturing {output:?} failed"))?;
    writeln!(std::io::stdout().lock(), "{report}")?;
    Ok(())
}

/// Captures the first output once, so an unauthorized install is reported
/// at startup rather than at the first idle check.
///
/// # Errors
///
/// The capture's error, or [`BackendError::NotFound`] when there are no
/// outputs.
pub async fn startup_check(capture: &KwinCapture) -> Result<CaptureReport, BackendError> {
    let outputs = capture.outputs().await?;
    let first = outputs
        .first()
        .ok_or_else(|| BackendError::NotFound("the compositor reports no outputs".into()))?;
    check_output(capture, &first.name, 1).await
}

#[cfg(test)]
mod tests {
    use stillwatch_core::luma::PixelFormat;

    use super::*;

    fn report(scale: Option<f64>) -> CaptureReport {
        CaptureReport {
            output: "HDMI-A-1".into(),
            meta: FrameMeta {
                format: PixelFormat::Argb32Premultiplied,
                width: 3840,
                height: 2160,
                stride: 15360,
                scale,
                screen: Some("HDMI-A-1".into()),
            },
            grid_width: 480,
            grid_height: 270,
            elapsed: Duration::from_millis(42),
        }
    }

    #[test]
    fn report_shows_dimensions_and_timing_only() {
        assert_eq!(
            report(Some(1.5)).to_string(),
            "HDMI-A-1: 3840x2160 Argb32Premultiplied, stride 15360, scale 1.5 \
             -> 480x270 luma grid in 42 ms"
        );
        assert_eq!(
            report(None).to_string(),
            "HDMI-A-1: 3840x2160 Argb32Premultiplied, stride 15360 -> 480x270 luma grid in 42 ms"
        );
    }
}
