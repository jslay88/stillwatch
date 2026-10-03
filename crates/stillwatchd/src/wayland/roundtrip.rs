//! An async roundtrip on an [`EventPump`], for states that delegate
//! `wl_callback` to [`Synced`].

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use stillwatch_core::backend::BackendError;
use wayland_client::protocol::wl_callback::{self, WlCallback};
use wayland_client::{Connection, Dispatch, QueueHandle};

use super::EventPump;

/// User data for a `wl_display.sync` callback: set once the compositor has
/// handled every request sent before it.
///
/// A state opts in with
/// `delegate_dispatch!(MyState: [WlCallback: Synced] => Synced);`.
#[derive(Debug, Clone, Default)]
pub struct Synced(Arc<AtomicBool>);

impl Synced {
    fn is_done(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

impl<S> Dispatch<WlCallback, Self, S> for Synced
where
    S: Dispatch<WlCallback, Self>,
{
    fn event(
        _: &mut S,
        _: &WlCallback,
        event: wl_callback::Event,
        synced: &Self,
        _: &Connection,
        _: &QueueHandle<S>,
    ) {
        if let wl_callback::Event::Done { .. } = event {
            synced.0.store(true, Ordering::Release);
        }
    }
}

impl<S> EventPump<S>
where
    S: Dispatch<WlCallback, Synced> + 'static,
{
    /// Sends `wl_display.sync` and dispatches events until it's answered, so
    /// everything the compositor sent in reply to earlier requests has been
    /// handled.
    ///
    /// # Errors
    ///
    /// As [`turn`](Self::turn).
    pub async fn roundtrip(
        &mut self,
        conn: &Connection,
        state: &mut S,
    ) -> Result<(), BackendError> {
        let synced = Synced::default();
        conn.display().sync(&self.handle(), synced.clone());
        self.run_until(state, |_| synced.is_done()).await
    }
}
