//! Tray, settings window, and prompt mode for Stillwatch.
//!
//! The window is an iced daemon: it stays up with no window until the tray
//! or a second invocation asks for one, and it keeps running after the last
//! window closes. The settings page is generated from the schema. Calibration,
//! history, service, and the prompt dialog are still placeholders. The tray
//! speaks D-Bus to `stillwatchd` and does not need the daemon in order to open.

mod app;
mod args;
mod boot;
mod bus;
mod daemon;
mod edit_msg;
mod error;
mod icons;
mod instance;
mod launch;
mod model;
mod page;
mod presets;
mod session;
mod settings;
mod shell;
mod tray;
mod tray_service;
mod view;
mod windows;

pub use args::Cli;
pub use daemon::{dispatch, watch};
pub use edit_msg::{FieldChange, RegionPart, RestoreScope, SettingsMsg};
pub use instance::{Claim, GUI_BUS_NAME, claim};
pub use launch::LaunchMode;
pub use model::update;
pub use shell::{DaemonCall, DaemonEvent, Link, Message, Shell, Snapshot, TrayAction, Visibility};

use stillwatch_ipc::logging::{self, LogTarget};

/// Log level when neither `--log-level` nor `RUST_LOG` is set.
const DEFAULT_LOG_LEVEL: &str = "warn";

/// Starts the tray and, when asked, the settings or prompt window.
///
/// A second process with the same bus name asks this one to focus the window
/// and then returns.
///
/// # Errors
///
/// Returns a logging or iced failure. A missing daemon is not a failure: the
/// window still opens and the tray shows that nothing is running.
pub fn run(cli: &Cli) -> anyhow::Result<()> {
    logging::init_with_override(
        cli.log_level.as_deref(),
        DEFAULT_LOG_LEVEL,
        LogTarget::Stderr,
    )?;
    app::run(cli)?;
    Ok(())
}
