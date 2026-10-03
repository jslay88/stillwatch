//! Service page: the user unit, tray autostart, and panel care.

mod autostart;
mod journal;
mod systemd;
mod unit;
mod view;

#[cfg(test)]
mod tests;

pub use systemd::{
    UNIT_NAME, UnitQuery, disable_unit, enable_unit, query_unit, restart_unit, start_unit,
    stop_unit,
};
pub use unit::{RunState, UnitView};
pub(crate) use view::page;

use stillwatch_ipc::status::PanelCareStatus;
use tokio::sync::mpsc;
use zbus::Connection;

use crate::shell::{DaemonCall, DaemonEvent};

pub(crate) use journal::open_journal;

/// What the page asked systemd to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitOp {
    /// `StartUnit`.
    Start,
    /// `StopUnit`.
    Stop,
    /// `RestartUnit`.
    Restart,
    /// Enable at login.
    Enable,
    /// Disable at login.
    Disable,
}

/// A service-page input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SvcMsg {
    /// Start the unit.
    Start,
    /// Stop the unit.
    Stop,
    /// Restart the unit.
    Restart,
    /// Enable the unit for login.
    Enable,
    /// Disable the unit.
    Disable,
    /// Write or remove the tray autostart file.
    Autostart(bool),
    /// Open the full journal.
    OpenLog,
    /// A unit query finished.
    Unit(UnitView),
    /// Whether the autostart file is present.
    AutostartState(bool),
}

/// Unit state and the tray autostart file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ServicePage {
    /// Last unit query.
    pub unit: UnitView,
    /// `None` until the autostart file has been checked.
    pub autostart: Option<bool>,
}

/// Applies `message` and returns the calls to make.
#[must_use]
pub fn apply(page: &mut ServicePage, message: SvcMsg) -> Vec<DaemonCall> {
    match message {
        SvcMsg::Start => vec![DaemonCall::Unit(UnitOp::Start)],
        SvcMsg::Stop => vec![DaemonCall::Unit(UnitOp::Stop)],
        SvcMsg::Restart => vec![DaemonCall::Unit(UnitOp::Restart)],
        SvcMsg::Enable => vec![DaemonCall::Unit(UnitOp::Enable)],
        SvcMsg::Disable => vec![DaemonCall::Unit(UnitOp::Disable)],
        SvcMsg::Autostart(enabled) => vec![DaemonCall::SetAutostart(enabled)],
        SvcMsg::OpenLog => vec![DaemonCall::OpenJournal],
        SvcMsg::Unit(unit) => {
            page.unit = unit;
            Vec::new()
        }
        SvcMsg::AutostartState(enabled) => {
            page.autostart = Some(enabled);
            Vec::new()
        }
    }
}

/// Screen-on time, last standby, and overlay uses.
///
/// `None` means the daemon has not reported panel care.
#[must_use]
pub fn panel_lines(care: Option<&PanelCareStatus>) -> Vec<String> {
    let Some(care) = care else {
        return vec!["Panel care has not reported yet.".to_owned()];
    };
    let standby = match care.last_standby {
        Some(at) => at.to_string(),
        None => "never".to_owned(),
    };
    vec![
        format!("Screen on: {}", screen_on(care.screen_on_seconds)),
        format!("Last standby: {standby}"),
        format!("Overlay uses: {}", care.overlay_uses),
    ]
}

fn screen_on(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    format!("{hours}h {minutes}m")
}

/// Reads the unit and sends [`DaemonEvent::Unit`].
pub(crate) async fn refresh_unit(conn: &Connection, events: &mpsc::Sender<DaemonEvent>) {
    let view = match systemd::query_view(conn).await {
        Ok(view) => view,
        Err(err) => UnitView::Error(err),
    };
    let _ = events.send(DaemonEvent::Unit(view)).await;
}

/// Runs `op`, then refreshes the unit.
pub(crate) async fn change_unit(conn: &Connection, op: UnitOp, events: &mpsc::Sender<DaemonEvent>) {
    let result = match op {
        UnitOp::Start => start_unit(conn).await,
        UnitOp::Stop => stop_unit(conn).await,
        UnitOp::Restart => restart_unit(conn).await,
        UnitOp::Enable => enable_unit(conn).await,
        UnitOp::Disable => disable_unit(conn).await,
    };
    if let Err(err) = result {
        let _ = events.send(DaemonEvent::CallFailed(err)).await;
    }
    refresh_unit(conn, events).await;
}

/// Sends whether the tray autostart file exists.
pub(crate) async fn read_autostart(events: &mpsc::Sender<DaemonEvent>) {
    let enabled = autostart::user_config_home().is_some_and(|home| autostart::is_enabled(&home));
    let _ = events.send(DaemonEvent::Autostart(enabled)).await;
}

/// Writes or removes the tray autostart file.
pub(crate) async fn write_autostart(enabled: bool, events: &mpsc::Sender<DaemonEvent>) {
    let Some(home) = autostart::user_config_home() else {
        let _ = events
            .send(DaemonEvent::CallFailed(
                "can't find the config directory (is $HOME set?)".to_owned(),
            ))
            .await;
        return;
    };
    match autostart::set_enabled(&home, enabled) {
        Ok(()) => {
            let _ = events.send(DaemonEvent::Autostart(enabled)).await;
        }
        Err(err) => {
            let _ = events.send(DaemonEvent::CallFailed(err.to_string())).await;
        }
    }
}
