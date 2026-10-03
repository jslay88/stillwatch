//! User systemd manager calls for `stillwatch.service`.
//!
//! The connection is whatever bus the GUI is on. Tests pass a private bus
//! with a fake manager. This never shells out to `systemctl`.

use zbus::Connection;
use zbus::zvariant::OwnedObjectPath;

use super::journal::recent_lines;
use super::unit::UnitView;

const DEST: &str = "org.freedesktop.systemd1";
const MANAGER_PATH: &str = "/org/freedesktop/systemd1";
const MANAGER_IFACE: &str = "org.freedesktop.systemd1.Manager";
const UNIT_IFACE: &str = "org.freedesktop.systemd1.Unit";

/// Unit name the page manages.
pub const UNIT_NAME: &str = "stillwatch.service";

const MODE: &str = "replace";
const NO_MANAGER: &str = "The systemd user manager is not available on this bus.";
const NOT_INSTALLED: &str = "stillwatch.service is not installed.";

/// What [`query_unit`] learned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnitQuery {
    /// No unit file.
    Missing,
    /// File state and `ActiveState`.
    Present {
        /// `ActiveState`.
        active: String,
        /// `GetUnitFileState`.
        file_state: String,
    },
}

/// `GetUnitFileState` plus `ActiveState`, or [`UnitQuery::Missing`].
///
/// # Errors
///
/// Returns a message when the user manager is missing or the call fails for
/// another reason. A missing unit file is [`Ok`] of [`UnitQuery::Missing`].
pub async fn query_unit(conn: &Connection) -> Result<UnitQuery, String> {
    match call_string(conn, "GetUnitFileState", UNIT_NAME).await {
        Ok(state) if state == "not-found" => Ok(UnitQuery::Missing),
        Ok(file_state) => {
            let path = call_path(conn, "LoadUnit", UNIT_NAME)
                .await
                .map_err(|err| explain(&err))?;
            let active = active_state(conn, &path)
                .await
                .map_err(|err| explain(&err))?;
            Ok(UnitQuery::Present { active, file_state })
        }
        Err(err) => match classify(&err.to_string()) {
            Class::Missing => Ok(UnitQuery::Missing),
            Class::NoManager => Err(NO_MANAGER.to_owned()),
            Class::Other => Err(err.to_string()),
        },
    }
}

/// Queries the unit and, when it is failed, attaches recent journal lines.
pub(crate) async fn query_view(conn: &Connection) -> Result<UnitView, String> {
    Ok(match query_unit(conn).await? {
        UnitQuery::Missing => UnitView::Missing,
        UnitQuery::Present { active, file_state } => {
            let journal = if active == "failed" {
                recent_lines()
            } else {
                Vec::new()
            };
            UnitView::ready(&active, &file_state, journal)
        }
    })
}

/// `StartUnit` with mode `replace`.
///
/// # Errors
///
/// Returns a message when systemd refuses the call.
pub async fn start_unit(conn: &Connection) -> Result<(), String> {
    job(conn, "StartUnit").await
}

/// `StopUnit` with mode `replace`.
///
/// # Errors
///
/// Returns a message when systemd refuses the call.
pub async fn stop_unit(conn: &Connection) -> Result<(), String> {
    job(conn, "StopUnit").await
}

/// `RestartUnit` with mode `replace`.
///
/// # Errors
///
/// Returns a message when systemd refuses the call.
pub async fn restart_unit(conn: &Connection) -> Result<(), String> {
    job(conn, "RestartUnit").await
}

/// `EnableUnitFiles` for this unit, then `Reload` so the manager sees it.
///
/// # Errors
///
/// Returns a message when systemd refuses the call.
pub async fn enable_unit(conn: &Connection) -> Result<(), String> {
    unit_files(conn, "EnableUnitFiles", true).await?;
    reload_manager(conn).await
}

/// `DisableUnitFiles` for this unit, then `Reload`.
///
/// # Errors
///
/// Returns a message when systemd refuses the call.
pub async fn disable_unit(conn: &Connection) -> Result<(), String> {
    unit_files(conn, "DisableUnitFiles", false).await?;
    reload_manager(conn).await
}

async fn job(conn: &Connection, method: &str) -> Result<(), String> {
    let reply = conn
        .call_method(
            Some(DEST),
            MANAGER_PATH,
            Some(MANAGER_IFACE),
            method,
            &(UNIT_NAME, MODE),
        )
        .await
        .map_err(|err| explain(&err))?;
    let _path: OwnedObjectPath = reply.body().deserialize().map_err(|err| explain(&err))?;
    Ok(())
}

async fn unit_files(conn: &Connection, method: &str, enable: bool) -> Result<(), String> {
    let names = [UNIT_NAME];
    let result = if enable {
        conn.call_method(
            Some(DEST),
            MANAGER_PATH,
            Some(MANAGER_IFACE),
            method,
            &(&names[..], false, false),
        )
        .await
    } else {
        conn.call_method(
            Some(DEST),
            MANAGER_PATH,
            Some(MANAGER_IFACE),
            method,
            &(&names[..], false),
        )
        .await
    };
    result.map(|_| ()).map_err(|err| explain(&err))
}

async fn reload_manager(conn: &Connection) -> Result<(), String> {
    conn.call_method(Some(DEST), MANAGER_PATH, Some(MANAGER_IFACE), "Reload", &())
        .await
        .map(|_| ())
        .map_err(|err| explain(&err))
}

async fn call_string(conn: &Connection, method: &str, name: &str) -> Result<String, zbus::Error> {
    named(conn, method, name).await?.body().deserialize()
}

async fn call_path(
    conn: &Connection,
    method: &str,
    name: &str,
) -> Result<OwnedObjectPath, zbus::Error> {
    named(conn, method, name).await?.body().deserialize()
}

async fn named(conn: &Connection, method: &str, name: &str) -> zbus::Result<zbus::Message> {
    conn.call_method(
        Some(DEST),
        MANAGER_PATH,
        Some(MANAGER_IFACE),
        method,
        &(name,),
    )
    .await
}

async fn active_state(conn: &Connection, path: &OwnedObjectPath) -> Result<String, zbus::Error> {
    let reply = conn
        .call_method(
            Some(DEST),
            path.as_ref(),
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &(UNIT_IFACE, "ActiveState"),
        )
        .await?;
    let value: zbus::zvariant::OwnedValue = reply.body().deserialize()?;
    String::try_from(value).map_err(|err| zbus::Error::Failure(err.to_string()))
}

fn explain(err: &zbus::Error) -> String {
    match classify(&err.to_string()) {
        Class::Missing => NOT_INSTALLED.to_owned(),
        Class::NoManager => NO_MANAGER.to_owned(),
        Class::Other => err.to_string(),
    }
}

enum Class {
    Missing,
    NoManager,
    Other,
}

fn classify(err: &str) -> Class {
    if err.contains("NoSuchUnit") || err.contains("NoSuchFile") || err.contains("FileNotFound") {
        Class::Missing
    } else if err.contains("ServiceUnknown") || err.contains("NameHasNoOwner") {
        Class::NoManager
    } else {
        Class::Other
    }
}
