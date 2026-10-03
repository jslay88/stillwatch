//! Which D-Bus names are on the buses we were given.
//!
//! `ScreenCast` is detected from the portal's introspection. Nothing here calls
//! `CreateSession`, `SelectSources`, or `Start`, so this does not open a
//! permission dialog. `ScreenShot2` is detected from its name and `Version`,
//! not from a capture.

use zbus::Connection;
use zbus::fdo::DBusProxy;
use zbus::names::BusName;
use zbus::zvariant::OwnedValue;

use super::facts::{DbusFacts, KwinBus, SessionServices};

/// `org.kde.KWin`.
pub const KWIN: &str = "org.kde.KWin";
/// `org.kde.KWin.ScreenShot2`.
pub const SCREENSHOT2: &str = "org.kde.KWin.ScreenShot2";
const SCREENSHOT2_PATH: &str = "/org/kde/KWin/ScreenShot2";
/// `org.freedesktop.portal.Desktop`.
pub const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const PORTAL_SCREENCAST: &str = "org.freedesktop.portal.ScreenCast";
/// `org.freedesktop.Notifications`.
pub const NOTIFICATIONS: &str = "org.freedesktop.Notifications";
/// `org.freedesktop.ScreenSaver`.
pub const SCREENSAVER: &str = "org.freedesktop.ScreenSaver";
/// `org.freedesktop.login1`.
pub const LOGIND: &str = "org.freedesktop.login1";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const INTROSPECTABLE: &str = "org.freedesktop.DBus.Introspectable";

/// Names on the session `conn`. logind is left false; the caller fills it
/// from the system bus. Authorization is left false; the caller fills it
/// from `.desktop` files.
pub async fn read_session(conn: &Connection) -> DbusFacts {
    let present = owned(conn, KWIN).await;
    let screenshot2 = owned(conn, SCREENSHOT2).await;
    DbusFacts {
        kwin: KwinBus {
            present,
            screenshot2,
            screenshot2_authorized: false,
            screenshot2_version: if screenshot2 {
                screenshot_version(conn).await
            } else {
                None
            },
        },
        portal_screencast: portal_screencast(conn).await,
        notifications: owned(conn, NOTIFICATIONS).await,
        session: SessionServices {
            logind: false,
            screensaver: owned(conn, SCREENSAVER).await,
        },
    }
}

/// Whether `org.freedesktop.login1` has an owner on `conn`.
pub async fn logind_present(conn: &Connection) -> bool {
    owned(conn, LOGIND).await
}

async fn owned(conn: &Connection, name: &str) -> bool {
    let Ok(dbus) = DBusProxy::new(conn).await else {
        return false;
    };
    let Ok(name) = BusName::try_from(name) else {
        return false;
    };
    dbus.name_has_owner(name).await.unwrap_or(false)
}

async fn screenshot_version(conn: &Connection) -> Option<u32> {
    let reply = conn
        .call_method(
            Some(SCREENSHOT2),
            SCREENSHOT2_PATH,
            Some(PROPERTIES),
            "Get",
            &(SCREENSHOT2, "Version"),
        )
        .await
        .ok()?;
    let value: OwnedValue = reply.body().deserialize().ok()?;
    as_u32(&value)
}

async fn portal_screencast(conn: &Connection) -> bool {
    if !owned(conn, PORTAL).await {
        return false;
    }
    let reply = conn
        .call_method(
            Some(PORTAL),
            PORTAL_PATH,
            Some(INTROSPECTABLE),
            "Introspect",
            &(),
        )
        .await
        .ok();
    reply
        .and_then(|reply| reply.body().deserialize::<String>().ok())
        .is_some_and(|xml| xml.contains(PORTAL_SCREENCAST))
}

fn as_u32(value: &OwnedValue) -> Option<u32> {
    value.downcast_ref::<u32>().ok()
}

#[cfg(test)]
mod tests {
    use zbus::zvariant;

    use super::*;

    #[test]
    fn version_property_is_a_u32() {
        let value = OwnedValue::try_from(zvariant::Value::from(5_u32)).unwrap();
        assert_eq!(as_u32(&value), Some(5));
        let text = OwnedValue::try_from(zvariant::Value::from("nope")).unwrap();
        assert_eq!(as_u32(&text), None);
    }
}
