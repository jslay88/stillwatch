//! The Stillwatch daemon.
//!
//! For now it sets up logging, reports where its config lives, and runs until
//! SIGTERM or SIGINT. The idle, capture, and action loop plugs in here later.

pub mod args;
pub mod idle;
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
    let config = args.config_path()?;
    let mut signals = Signals::install().context("can't install signal handlers")?;

    tracing::info!(
        version = VERSION,
        config = %config.display(),
        config_exists = config.exists(),
        ?sink,
        "stillwatchd started"
    );
    let signal = signals::wait_for_shutdown(&mut signals).await;
    tracing::info!(?signal, "stillwatchd stopping");
    Ok(())
}
