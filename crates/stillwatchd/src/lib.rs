//! The Stillwatch daemon.
//!
//! `main` parses arguments. [`daemon`] owns the event loop that connects the
//! backends to the state machine.

pub mod action;
pub mod activity;
pub mod args;
pub mod capture;
pub mod clock;
pub mod config_watch;
mod daemon;
mod dbus;
pub mod gamepad;
pub mod history;
mod hotplug;
pub mod idle;
pub mod media;
pub mod outputs;
pub mod overlay;
pub mod panel;
mod peer;
pub mod platform;
pub mod probe;
pub mod process;
pub mod prompt;
pub mod service;
pub mod session;
pub mod signals;
pub mod supervise;
mod wayland;

use anyhow::Context as _;
use stillwatch_ipc::logging::{self, LogTarget};

pub use args::Args;
use signals::Signals;

/// The daemon version, as declared in `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Runs the daemon until it's asked to stop.
///
/// # Errors
///
/// Fails if logging can't be initialized, the config path can't be resolved,
/// or signal handlers can't be installed.
pub async fn run(args: Args) -> anyhow::Result<()> {
    let sink = logging::init_with_override(
        args.log_level.as_deref(),
        logging::DEFAULT_LEVEL,
        LogTarget::Auto,
    )?;
    if let Some(output) = &args.capture_check {
        return capture::kwin::run_check(output).await;
    }
    if args.probe {
        return probe::run_cli(&args).await;
    }
    let config = args.config_path()?;
    let mut signals = Signals::install().context("can't install signal handlers")?;

    tracing::info!(
        version = VERSION,
        config = %config.display(),
        config_exists = config.exists(),
        ?sink,
        "stillwatchd started"
    );
    daemon::run(config, &mut signals).await
}
