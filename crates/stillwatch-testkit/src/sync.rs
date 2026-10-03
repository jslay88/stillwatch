use std::sync::{Mutex, MutexGuard, PoisonError};

/// Locks `mutex`, carrying on past a panic in another test thread.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
