use std::sync::Mutex;

use super::{CallLog, Script};
use crate::backend::{BackendError, BackendFuture, ScreenCapture};
use crate::luma::{LumaGrid, OutputInfo};
use crate::sync::lock;

/// A [`ScreenCapture`] that returns queued grids and errors and records each
/// capture request.
///
/// `capture_luma` takes the next queued result regardless of output; with the
/// queue empty it returns `BackendError::Unavailable`.
#[derive(Debug, Default)]
pub struct MockCapture {
    outputs: Mutex<Vec<OutputInfo>>,
    captures: Script<LumaGrid>,
    requests: CallLog<(String, u32)>,
}

impl MockCapture {
    /// A capture backend with no outputs and nothing queued.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces what `outputs` returns.
    pub fn set_outputs(&self, outputs: Vec<OutputInfo>) {
        *lock(&self.outputs) = outputs;
    }

    /// Queues a successful capture.
    pub fn push_grid(&self, grid: LumaGrid) {
        self.captures.push(Ok(grid));
    }

    /// Queues a failed capture.
    pub fn push_error(&self, error: BackendError) {
        self.captures.push(Err(error));
    }

    /// Each `(output, downscale_width)` passed to `capture_luma`, oldest first.
    #[must_use]
    pub fn requests(&self) -> Vec<(String, u32)> {
        self.requests.snapshot()
    }
}

impl ScreenCapture for MockCapture {
    fn outputs(&self) -> BackendFuture<'_, Vec<OutputInfo>> {
        let outputs = lock(&self.outputs).clone();
        Box::pin(std::future::ready(Ok(outputs)))
    }

    fn capture_luma<'a>(
        &'a self,
        output: &'a str,
        downscale_width: u32,
    ) -> BackendFuture<'a, LumaGrid> {
        self.requests.push((output.to_owned(), downscale_width));
        let result = self.captures.pop().unwrap_or_else(|| {
            Err(BackendError::Unavailable(
                "mock capture queue is empty".into(),
            ))
        });
        Box::pin(std::future::ready(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mocks::now_or_never;

    #[test]
    fn returns_queued_results_and_records_requests() {
        let capture = MockCapture::new();
        let output = OutputInfo::new("HDMI-A-1", 3840, 2160);
        capture.set_outputs(vec![output.clone()]);
        let grid = LumaGrid::filled(4, 2, 128).unwrap();
        capture.push_grid(grid.clone());
        capture.push_error(BackendError::PermissionDenied("kwin".into()));

        assert_eq!(now_or_never(capture.outputs()), Some(Ok(vec![output])));
        assert_eq!(
            now_or_never(capture.capture_luma("HDMI-A-1", 480)),
            Some(Ok(grid))
        );
        assert_eq!(
            now_or_never(capture.capture_luma("DP-1", 240)),
            Some(Err(BackendError::PermissionDenied("kwin".into())))
        );
        assert!(matches!(
            now_or_never(capture.capture_luma("DP-1", 240)),
            Some(Err(BackendError::Unavailable(_)))
        ));
        assert_eq!(
            capture.requests(),
            vec![
                ("HDMI-A-1".into(), 480),
                ("DP-1".into(), 240),
                ("DP-1".into(), 240)
            ]
        );
    }
}
