//! The one GUI process on the bus. A second process calls `Activate` here
//! and exits.

mod dbus;

use tokio::sync::mpsc;
use zbus::Connection;
use zbus::fdo::RequestNameFlags;
use zbus::proxy::CacheProperties;

use self::dbus::{BUS_NAME, GuiObject, StillwatchGuiProxy};
use crate::error::Error;
use crate::launch::LaunchMode;

pub use self::dbus::{BUS_NAME as GUI_BUS_NAME, OBJECT_PATH as GUI_OBJECT_PATH};

/// Whether this process owns [`GUI_BUS_NAME`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Claim {
    /// This process serves `Activate` and should run the tray.
    Primary,
    /// Another process is running and was asked to show `mode`.
    HandedOff,
}

/// Serves the GUI name ([`GUI_BUS_NAME`] at [`GUI_OBJECT_PATH`]), or asks the
/// process that already owns it to show `mode`.
///
/// The object is registered before the name is requested, so a second
/// process that loses the race can call `Activate` as soon as the name exists.
///
/// # Errors
///
/// Returns a D-Bus error when the object can't be served or the activate
/// call to the existing instance fails. [`zbus::Error::NameTaken`] is not an
/// error: it becomes [`Claim::HandedOff`].
pub async fn claim(
    connection: &Connection,
    mode: LaunchMode,
    activations: mpsc::Sender<LaunchMode>,
) -> Result<Claim, Error> {
    let object = GuiObject { activations };
    if !connection
        .object_server()
        .at(GUI_OBJECT_PATH, object)
        .await?
    {
        return Err(Error::Bus(zbus::Error::Failure(format!(
            "{GUI_OBJECT_PATH} is already served"
        ))));
    }
    match connection
        .request_name_with_flags(BUS_NAME, RequestNameFlags::DoNotQueue.into())
        .await
    {
        Ok(_) => Ok(Claim::Primary),
        Err(zbus::Error::NameTaken) => {
            let proxy = StillwatchGuiProxy::builder(connection)
                .cache_properties(CacheProperties::No)
                .build()
                .await?;
            proxy.activate(mode.as_str()).await?;
            Ok(Claim::HandedOff)
        }
        Err(err) => Err(err.into()),
    }
}
