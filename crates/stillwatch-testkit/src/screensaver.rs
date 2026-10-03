//! A fake `org.freedesktop.ScreenSaver`, as Plasma's `KWin` serves it.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use zbus::Connection;
use zbus::connection::Builder;
use zbus::object_server::SignalEmitter;

use self::object::{ScreenSaver, ScreenSaverSignals as _, State};
use crate::{Error, PrivateBus};

/// The screensaver's bus name.
pub const SERVICE: &str = "org.freedesktop.ScreenSaver";

/// The object path from the freedesktop spec.
pub const OBJECT_PATH: &str = "/org/freedesktop/ScreenSaver";

/// A screensaver on its own connection, owning `org.freedesktop.ScreenSaver`.
///
/// `Lock` calls are counted, and can be refused to test a fallback.
#[derive(Debug)]
pub struct FakeScreenSaver {
    conn: Connection,
    state: Arc<State>,
}

impl FakeScreenSaver {
    /// Starts an inactive (unlocked) screensaver.
    ///
    /// # Errors
    ///
    /// Fails if the connection or the name request fails.
    pub async fn spawn(bus: &PrivateBus) -> Result<Self, Error> {
        let state = Arc::new(State::default());
        let conn = Builder::address(bus.address())?
            .serve_at(OBJECT_PATH, ScreenSaver(Arc::clone(&state)))?
            .name(SERVICE)?
            .build()
            .await?;
        Ok(Self { conn, state })
    }

    /// Sets `Active` and emits `ActiveChanged` even if it didn't change, so
    /// tests can send duplicates.
    ///
    /// # Errors
    ///
    /// Fails if the signal can't be sent.
    pub async fn set_active(&self, active: bool) -> Result<(), Error> {
        self.state.active.store(active, Ordering::SeqCst);
        let emitter = SignalEmitter::new(&self.conn, OBJECT_PATH)?;
        emitter.active_changed(active).await?;
        Ok(())
    }

    /// Whether the screensaver (lock screen) is active.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.state.active.load(Ordering::SeqCst)
    }

    /// Makes `Lock` fail with `AccessDenied` from now on, or work again.
    pub fn refuse_lock(&self, refuse: bool) {
        self.state.refuse_lock.store(refuse, Ordering::SeqCst);
    }

    /// How many times `Lock` was called, refused or not.
    #[must_use]
    pub fn lock_calls(&self) -> usize {
        self.state.lock_calls.load(Ordering::SeqCst)
    }

    /// Drops off the bus as if the screensaver exited.
    ///
    /// # Errors
    ///
    /// Fails if closing the socket fails.
    pub async fn exit(self) -> Result<(), Error> {
        Ok(self.conn.close().await?)
    }
}

// zbus generates an undocumented public signal trait for the interface, so
// it stays in a private module.
mod object {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use zbus::fdo;
    use zbus::object_server::SignalEmitter;

    #[derive(Debug, Default)]
    pub(super) struct State {
        pub active: AtomicBool,
        pub refuse_lock: AtomicBool,
        pub lock_calls: AtomicUsize,
    }

    pub(super) struct ScreenSaver(pub Arc<State>);

    #[zbus::interface(name = "org.freedesktop.ScreenSaver")]
    impl ScreenSaver {
        fn get_active(&self) -> bool {
            self.0.active.load(Ordering::SeqCst)
        }

        /// Like Plasma: locks, turns `Active` on, and announces it.
        async fn lock(
            &self,
            #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        ) -> fdo::Result<()> {
            self.0.lock_calls.fetch_add(1, Ordering::SeqCst);
            if self.0.refuse_lock.load(Ordering::SeqCst) {
                return Err(fdo::Error::AccessDenied("locking is refused".into()));
            }
            if !self.0.active.swap(true, Ordering::SeqCst) {
                Self::active_changed(&emitter, true).await?;
            }
            Ok(())
        }

        /// The screensaver (lock screen) turned on or off.
        #[zbus(signal)]
        async fn active_changed(emitter: &SignalEmitter<'_>, active: bool) -> zbus::Result<()>;
    }
}
