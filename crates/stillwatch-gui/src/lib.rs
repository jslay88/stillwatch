//! Tray, settings window, and prompt mode for Stillwatch.
//!
//! The window is an iced daemon: it stays up with no window until the tray
//! or a second invocation asks for one, and it keeps running after the last
//! window closes. The settings page is generated from the schema. Calibration
//! is the heatmap. History lists past decisions. Service manages the user unit
//! and tray autostart. `prompt` is a short-lived dialog that answers over
//! D-Bus. The tray speaks D-Bus to `stillwatchd` and does not need the daemon
//! in order to open.

mod app;
mod args;
mod boot;
mod bus;
mod calibration;
mod daemon;
mod edit_msg;
mod error;
mod history;
mod icons;
mod instance;
mod launch;
mod model;
mod page;
mod presets;
mod prompt;
mod service;
mod session;
mod settings;
mod shell;
mod tray;
mod tray_service;
mod view;
mod windows;

pub use args::Cli;
pub use calibration::{CalMsg, Calibration, OutputHeat, ProbePace, ProbeView};
pub use daemon::{dispatch, watch};
pub use edit_msg::{FieldChange, PresetKind, RegionPart, RestoreScope, SettingsMsg};
pub use history::{
    HistMsg, HistoryLoad, HistoryPage, HistoryRow, HistorySource, KindFilter, TimeRange,
};
pub use instance::{Claim, GUI_BUS_NAME, claim};
pub use launch::LaunchMode;
pub use model::update;
pub use page::Page;
pub use prompt::{Dialog, Input, Note, Step, answer_prompt, update as update_prompt};
pub use service::{
    RunState, ServicePage, SvcMsg, UNIT_NAME, UnitOp, UnitQuery, UnitView, disable_unit,
    enable_unit, query_unit, restart_unit, start_unit, stop_unit,
};
pub use settings::{
    Catalog, GamepadSeen, KeyChange, PickerRow, activity_lit, gamepad_rows, output_rows,
    player_rows, player_value,
};
pub use shell::{DaemonCall, DaemonEvent, Link, Message, Shell, Snapshot, TrayAction, Visibility};

use stillwatch_ipc::logging::{self, LogTarget};

/// Log level when neither `--log-level` nor `RUST_LOG` is set.
const DEFAULT_LOG_LEVEL: &str = "warn";

/// Starts the tray and, when asked, the settings window.
///
/// `prompt` is a separate short-lived dialog. It returns `0` when an answer
/// was sent or the prompt had already resolved, `1` when a dismissal could
/// not be sent, and `2` when some other answer could not be sent. The tray
/// returns `0`.
///
/// A second process with the same bus name asks the running tray to focus
/// the window and then returns. The prompt dialog does not take that name.
///
/// # Errors
///
/// Returns a logging or iced failure. A missing daemon is not a failure: the
/// window still opens and the tray shows that nothing is running.
pub fn run(cli: &Cli) -> anyhow::Result<i32> {
    logging::init_with_override(
        cli.log_level.as_deref(),
        DEFAULT_LOG_LEVEL,
        LogTarget::Stderr,
    )?;
    if matches!(cli.command, Some(args::Command::Prompt { .. })) {
        prompt::run(cli)
    } else {
        app::run(cli)?;
        Ok(0)
    }
}
