//! One capture tick: outputs → luma grids → [`BlockDetector::observe`] →
//! [`ProbeSample`].

use stillwatch_core::backend::{BackendError, MediaPlayer, ScreenCapture};
use stillwatch_core::detector::BlockDetector;
use stillwatch_core::event::CaptureFrame;
use stillwatch_core::stats::DetectionStats;
use stillwatch_core::time::Clock;
use stillwatch_ipc::probe::{ProbeOutput, ProbeSample};

/// Captures every monitored output, feeds the detector, and builds a sample.
///
/// Sets the detector's output sizes so `ignore_regions` map to blocks. Luma
/// stays inside the detector; only states and percentages leave.
///
/// # Errors
///
/// Listing or capturing an output, or a `block_grid` that doesn't fit the
/// wire type.
pub async fn sample(
    capture: &dyn ScreenCapture,
    detector: &mut BlockDetector,
    clock: &dyn Clock,
    downscale_width: u32,
    playing: &[MediaPlayer],
) -> Result<ProbeSample, BackendError> {
    let outputs = capture.outputs().await?;
    detector.set_outputs(&outputs);
    let mut frames = Vec::new();
    for output in &outputs {
        if !detector.monitors(&output.name) {
            continue;
        }
        let grid = capture.capture_luma(&output.name, downscale_width).await?;
        frames.push(CaptureFrame {
            output: output.name.clone(),
            grid,
        });
    }
    from_stats(
        detector.observe(&frames, playing),
        detector,
        clock.wall_now(),
    )
}

fn from_stats(
    stats: DetectionStats,
    detector: &BlockDetector,
    at: jiff::Timestamp,
) -> Result<ProbeSample, BackendError> {
    let [col_count, row_count] = detector.grid();
    let columns = fit_u16(col_count, "columns")?;
    let rows = fit_u16(row_count, "rows")?;
    let outputs = stats
        .outputs
        .into_iter()
        .map(|output| {
            let (width, height) = detector.output_size(&output.output).unwrap_or((0, 0));
            ProbeOutput {
                blocks: detector.blocks(&output.output).unwrap_or(&[]).to_vec(),
                stats: output,
                columns,
                rows,
                width,
                height,
            }
        })
        .collect();
    Ok(ProbeSample {
        at,
        threshold: stats.threshold,
        stale: stats.stale,
        outputs,
    })
}

fn fit_u16(value: u32, what: &str) -> Result<u16, BackendError> {
    u16::try_from(value).map_err(|_| {
        BackendError::Protocol(format!("block_grid {what} {value} does not fit in u16"))
    })
}
