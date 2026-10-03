//! The one prompt notification that may be open, and closing it.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use super::proxy::{self, NotificationsProxy};

/// The open prompt: its id and the connection that showed it.
#[derive(Debug)]
struct Open {
    /// Which `show` call owns it.
    token: u64,
    id: u32,
    proxy: NotificationsProxy<'static>,
}

/// Holds the open prompt so `dismiss` and the next `show` can close it.
#[derive(Debug, Default)]
pub(crate) struct OpenSlot {
    current: Mutex<Option<Open>>,
    next_token: AtomicU64,
}

impl OpenSlot {
    /// Records a prompt that was just shown. The guard closes it when dropped
    /// unless it was closed or forgotten first.
    pub(crate) fn track(&self, proxy: NotificationsProxy<'static>, id: u32) -> OpenGuard<'_> {
        let token = self.next_token.fetch_add(1, Ordering::Relaxed);
        *self.lock() = Some(Open { token, id, proxy });
        OpenGuard { slot: self, token }
    }

    /// Closes the open prompt, if any.
    pub(crate) async fn close_current(&self) {
        let open = self.lock().take();
        if let Some(open) = open {
            proxy::close(&open.proxy, open.id).await;
        }
    }

    fn take_if(&self, token: u64) -> Option<Open> {
        let mut current = self.lock();
        if current.as_ref().is_some_and(|open| open.token == token) {
            current.take()
        } else {
            None
        }
    }

    fn lock(&self) -> MutexGuard<'_, Option<Open>> {
        self.current.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// One `show` call's claim on the open prompt.
#[derive(Debug)]
pub(crate) struct OpenGuard<'a> {
    slot: &'a OpenSlot,
    token: u64,
}

impl OpenGuard<'_> {
    /// Records that the prompt now has id `id`. `false` if it was closed
    /// meanwhile (dismissed, or replaced by a newer prompt).
    pub(crate) fn set_id(&self, id: u32) -> bool {
        let mut current = self.slot.lock();
        match current.as_mut() {
            Some(open) if open.token == self.token => {
                open.id = id;
                true
            }
            _ => false,
        }
    }

    /// Closes the prompt now.
    pub(crate) async fn close(self) {
        if let Some(open) = self.slot.take_if(self.token) {
            proxy::close(&open.proxy, open.id).await;
        }
    }

    /// Lets go of a prompt the server already closed.
    pub(crate) fn forget(self) {
        self.slot.take_if(self.token);
    }
}

impl Drop for OpenGuard<'_> {
    /// `show` was cancelled with its prompt still up. Closing needs a D-Bus
    /// call, so it runs as its own task.
    fn drop(&mut self) {
        let Some(open) = self.slot.take_if(self.token) else {
            return;
        };
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => {
                runtime.spawn(async move { proxy::close(&open.proxy, open.id).await });
            }
            Err(err) => tracing::warn!(id = open.id, %err, "can't close the prompt"),
        }
    }
}
