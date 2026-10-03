//! Single-instance D-Bus object.
//!
//! zbus generates the proxy and interface helpers without doc comments.

#![allow(missing_docs)]

use tokio::sync::mpsc;
use zbus::fdo;

use crate::launch::LaunchMode;

/// Well-known name of the running GUI.
pub const BUS_NAME: &str = "io.github.jslay88.Stillwatch.Gui";

/// Object path [`BUS_NAME`] serves `Activate` at.
pub const OBJECT_PATH: &str = "/io/github/jslay88/Stillwatch/Gui";

/// Receives `Activate` and forwards the mode to the shell.
pub struct GuiObject {
    /// Launch modes requested by later processes.
    pub activations: mpsc::Sender<LaunchMode>,
}

#[zbus::interface(name = "io.github.jslay88.Stillwatch.Gui1")]
impl GuiObject {
    async fn activate(&self, mode: &str) -> fdo::Result<()> {
        let mode = LaunchMode::parse(mode).map_err(fdo::Error::InvalidArgs)?;
        self.activations
            .send(mode)
            .await
            .map_err(|_| fdo::Error::Failed("the tray is closing".into()))?;
        Ok(())
    }
}

#[zbus::proxy(
    interface = "io.github.jslay88.Stillwatch.Gui1",
    default_service = "io.github.jslay88.Stillwatch.Gui",
    default_path = "/io/github/jslay88/Stillwatch/Gui"
)]
pub trait StillwatchGui {
    /// Asks the running GUI to show `mode` (`tray`, `settings`, or `prompt`).
    fn activate(&self, mode: &str) -> zbus::Result<()>;
}
