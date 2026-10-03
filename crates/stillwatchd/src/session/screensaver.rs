//! `org.freedesktop.ScreenSaver` on the session bus. On Plasma it belongs to
//! `KWin`, `Active` means the lock screen is up, and `Lock` locks right away.

use stillwatch_core::backend::BackendError;
use zbus::Connection;
use zbus::proxy::CacheProperties;

use crate::dbus::call_error;

pub(crate) const SERVICE: &str = "org.freedesktop.ScreenSaver";

#[zbus::proxy(
    interface = "org.freedesktop.ScreenSaver",
    default_service = "org.freedesktop.ScreenSaver",
    default_path = "/org/freedesktop/ScreenSaver"
)]
pub(crate) trait ScreenSaver {
    fn get_active(&self) -> zbus::Result<bool>;

    fn lock(&self) -> zbus::Result<()>;

    #[zbus(signal)]
    fn active_changed(&self, active: bool) -> zbus::Result<()>;
}

/// The screensaver proxy. It works whether or not anything owns the name
/// yet; signals start once something does.
pub(crate) async fn proxy(conn: &Connection) -> Result<ScreenSaverProxy<'static>, BackendError> {
    ScreenSaverProxy::builder(conn)
        .cache_properties(CacheProperties::No)
        .build()
        .await
        .map_err(error)
}

/// A failed screensaver call as a [`BackendError`].
pub(crate) fn error(err: zbus::Error) -> BackendError {
    call_error(SERVICE, err)
}
