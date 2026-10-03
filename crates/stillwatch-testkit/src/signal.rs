use std::borrow::Cow;
use std::collections::HashMap;

use zbus::Connection;
use zbus::fdo::Properties;
use zbus::names::InterfaceName;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::Value;

use crate::Error;

/// Emits `PropertiesChanged` for `interface` at `path` from `conn`.
pub(crate) async fn properties_changed(
    conn: &Connection,
    path: &str,
    interface: &str,
    changed: HashMap<&str, Value<'_>>,
    invalidated: &[&str],
) -> Result<(), Error> {
    let emitter = SignalEmitter::new(conn, path)?;
    let interface = InterfaceName::try_from(interface).map_err(zbus::Error::from)?;
    Properties::properties_changed(&emitter, interface, changed, Cow::Borrowed(invalidated))
        .await?;
    Ok(())
}
