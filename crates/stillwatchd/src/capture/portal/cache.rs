//! The latest luma grid per output. Raw frames are not stored.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use stillwatch_core::backend::BackendError;
use stillwatch_core::luma::LumaGrid;
use tokio::sync::Notify;

/// Latest downscaled frame for each connector, plus the width the stream should use.
pub(crate) struct FrameCache {
    grids: Mutex<HashMap<String, (u32, LumaGrid)>>,
    width: AtomicU32,
    notify: Notify,
    fault: Mutex<Option<BackendError>>,
}

impl FrameCache {
    pub(crate) fn new() -> Self {
        Self {
            grids: Mutex::default(),
            width: AtomicU32::new(1),
            notify: Notify::new(),
            fault: Mutex::default(),
        }
    }

    pub(crate) fn set_width(&self, width: u32) {
        self.width.store(width.max(1), Ordering::Relaxed);
    }

    pub(crate) fn width(&self) -> u32 {
        self.width.load(Ordering::Relaxed).max(1)
    }

    pub(crate) fn store(&self, output: &str, width: u32, grid: LumaGrid) {
        lock(&self.grids).insert(output.to_owned(), (width, grid));
        self.notify.notify_waiters();
    }

    pub(crate) fn get(&self, output: &str, width: u32) -> Option<LumaGrid> {
        lock(&self.grids)
            .get(output)
            .filter(|(got, _)| *got == width)
            .map(|(_, grid)| grid.clone())
    }

    pub(crate) fn fail(&self, error: BackendError) {
        *lock(&self.fault) = Some(error);
        self.notify.notify_waiters();
    }

    pub(crate) fn fault(&self) -> Option<BackendError> {
        lock(&self.fault).clone()
    }

    pub(crate) fn clear(&self) {
        lock(&self.grids).clear();
        *lock(&self.fault) = None;
    }

    pub(crate) fn notify(&self) -> &Notify {
        &self.notify
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
