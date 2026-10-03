//! JSON-line loop used by `stillwatchd --probe`.

use std::future::Future;
use std::io::Write;
use std::pin::pin;

use anyhow::Context as _;
use stillwatch_core::backend::ScreenCapture;
use stillwatch_core::detector::BlockDetector;
use stillwatch_core::time::Clock;
use stillwatch_ipc::json::to_json_lines;

use super::{Settings, sample};

/// Captures, observes, and writes one [`ProbeSample`](stillwatch_ipc::probe::ProbeSample)
/// JSON line per interval until `count` or `stop`.
///
/// # Errors
///
/// A capture failure, a sample that can't be encoded, or a write failure.
pub async fn run(
    capture: &dyn ScreenCapture,
    detector: &mut BlockDetector,
    clock: &dyn Clock,
    settings: &Settings,
    mut playing: impl FnMut() -> Vec<String>,
    out: &mut dyn Write,
    stop: impl Future<Output = ()>,
) -> anyhow::Result<()> {
    let mut stop = pin!(stop);
    let mut seen = 0;
    loop {
        let names = playing();
        let taken = sample(capture, detector, clock, settings.downscale_width, &names)
            .await
            .context("capturing a probe sample")?;
        out.write_all(to_json_lines(std::slice::from_ref(&taken))?.as_bytes())?;
        out.flush().context("can't write the probe sample")?;
        seen += 1;
        if settings.count.is_some_and(|count| seen >= count.get()) {
            return Ok(());
        }
        tokio::select! {
            () = &mut stop => return Ok(()),
            () = tokio::time::sleep(settings.interval) => {}
        }
    }
}
