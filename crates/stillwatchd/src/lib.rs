//! The Stillwatch daemon.
//!
//! For now it sets up logging, reports where its config lives, and runs until
//! SIGTERM or SIGINT. The idle, capture, and action loop plugs in here later.

pub mod action;
pub mod activity;
pub mod args;
pub mod capture;
pub mod clock;
pub mod config_watch;
mod dbus;
pub mod gamepad;
pub mod history;
pub mod idle;
pub mod media;
pub mod outputs;
pub mod overlay;
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
    tokio::spawn(check_kwin_capture());
    let signal = signals::wait_for_shutdown(&mut signals).await;
    tracing::info!(?signal, "stillwatchd stopping");
    Ok(())
}

/// Logs whether `KWin` `ScreenShot2` capture works, with remediation when it
/// isn't authorized. Without it Stillwatch runs on input idle alone.
async fn check_kwin_capture() {
    let result = match capture::kwin::KwinCapture::connect().await {
        Ok(kwin) => capture::kwin::startup_check(&kwin).await,
        Err(error) => Err(error),
    };
    match result {
        Ok(report) => tracing::info!(%report, "KWin ScreenShot2 capture works"),
        Err(error @ stillwatch_core::backend::BackendError::PermissionDenied(_)) => {
            tracing::warn!(%error, "KWin ScreenShot2 capture isn't authorized");
        }
        Err(error) => tracing::info!(%error, "KWin ScreenShot2 capture is unavailable"),
    }
}
