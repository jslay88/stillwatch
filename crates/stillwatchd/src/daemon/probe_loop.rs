//! Calibration probe. Uses its own detector so samples don't move the
//! state machine's block counters. [`probe::sample`](crate::probe::sample)
//! is the only capture path here; luma never leaves that function.

use std::sync::Arc;
use std::time::Duration;

use stillwatch_core::detector::BlockDetector;
use stillwatch_core::time::Clock;
use stillwatch_ipc::probe::ProbeSample;
use tokio::sync::mpsc;

use super::shared::{Shared, lock};

/// Sends samples until `tx` is dropped or the task is aborted.
///
/// The first sample is immediate, then one per `interval`, matching
/// `stillwatchd --probe`. A missing capture backend skips the sample.
pub(super) async fn run(
    shared: Arc<Shared>,
    clock: Arc<dyn Clock>,
    interval: Duration,
    tx: mpsc::Sender<ProbeSample>,
) {
    let mut detector = BlockDetector::new(&lock(&shared.config));
    let mut logged = false;
    loop {
        if tx.is_closed() {
            break;
        }
        sample(&shared, clock.as_ref(), &mut detector, &tx, &mut logged).await;
        tokio::time::sleep(interval).await;
    }
}

async fn sample(
    shared: &Shared,
    clock: &dyn Clock,
    detector: &mut BlockDetector,
    tx: &mpsc::Sender<ProbeSample>,
    logged: &mut bool,
) {
    let config = lock(&shared.config).clone();
    let playing = lock(&shared.playing).clone();
    let capture = lock(&shared.capture).clone();
    let Some(capture) = capture else {
        if !*logged {
            tracing::info!("probe has no capture backend");
            *logged = true;
        }
        return;
    };
    detector.apply_config(&config);
    match crate::probe::sample(
        capture.as_ref(),
        detector,
        clock,
        config.stale.downscale_width,
        &playing,
    )
    .await
    {
        Ok(sample) => {
            if tx.send(sample).await.is_err() {
                tracing::debug!("probe subscriber is gone");
            }
        }
        Err(error) => tracing::warn!(%error, "probe sample failed"),
    }
}
