//! Capture → detector → [`ProbeSample`](stillwatch_ipc::probe::ProbeSample) loop.
//!
//! `stillwatchd --probe` prints one JSON line per sample. JUS-36 should call
//! [`sample`] on the same interval for D-Bus `probe()` rather than capturing
//! on its own.

mod emit;
mod playing;
mod sample;

use std::num::NonZeroUsize;
use std::path::Path;
use std::time::Duration;

use anyhow::Context as _;
use stillwatch_core::backend::MediaWatcher as _;
use stillwatch_core::config::{Config, ConfigError, StaleConfig};
use stillwatch_core::detector::BlockDetector;
use stillwatch_ipc::config_file;
use stillwatch_ipc::probe::MIN_PROBE_INTERVAL_MS;

use crate::args::Args;
use crate::capture::kwin::KwinCapture;
use crate::clock::TokioClock;
use crate::media::MprisWatcher;
use crate::signals::{self, Signals};

pub use emit::run;
pub use playing::Playing;
pub use sample::sample;

/// How often the loop captures and how many samples it takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    /// Sleep between samples. Must be at least [`MIN_PROBE_INTERVAL_MS`].
    pub interval: Duration,
    /// Stop after this many samples. `None` runs until `stop`.
    pub count: Option<NonZeroUsize>,
    /// Passed to [`ScreenCapture::capture_luma`](stillwatch_core::backend::ScreenCapture::capture_luma).
    pub downscale_width: u32,
}

/// `stillwatchd --probe`: load config, capture, print JSON lines.
///
/// # Errors
///
/// Fails if the config is invalid, the interval is below
/// [`MIN_PROBE_INTERVAL_MS`], `KWin` can't capture, a sample can't be
/// written, or signal handlers can't be installed.
pub async fn run_cli(args: &Args) -> anyhow::Result<()> {
    let config = load_config(&args.config_path()?)?;
    let settings = Settings {
        interval: interval(args.interval, &config.stale)?,
        count: args.count,
        downscale_width: config.stale.downscale_width,
    };
    let capture = KwinCapture::connect().await?;
    let mut detector = BlockDetector::new(&config);
    let playing = Playing::default();
    watch_media(&playing);
    let mut signals = Signals::install().context("can't install signal handlers")?;
    let stop = async move {
        signals::wait_for_shutdown(&mut signals).await;
    };
    let mut out = std::io::stdout().lock();
    run(
        &capture,
        &mut detector,
        &TokioClock,
        &settings,
        || playing.snapshot(),
        &mut out,
        stop,
    )
    .await
}

/// `override` if given, otherwise `stale.check_interval_seconds`.
///
/// # Errors
///
/// The interval is shorter than [`MIN_PROBE_INTERVAL_MS`].
pub fn interval(
    override_interval: Option<Duration>,
    stale: &StaleConfig,
) -> Result<Duration, anyhow::Error> {
    let interval = override_interval
        .unwrap_or_else(|| Duration::from_secs(u64::from(stale.check_interval_seconds)));
    let minimum = Duration::from_millis(u64::from(MIN_PROBE_INTERVAL_MS));
    if interval < minimum {
        anyhow::bail!("probe interval must be at least {MIN_PROBE_INTERVAL_MS} ms");
    }
    Ok(interval)
}

pub(crate) fn load_config(path: &Path) -> anyhow::Result<Config> {
    match config_file::load(path) {
        Ok(outcome) => Ok(outcome.config),
        Err(ConfigError::NotFound { .. }) => Ok(Config::default()),
        Err(error) => Err(error).context("can't load the config"),
    }
}

fn watch_media(playing: &Playing) {
    let watcher = MprisWatcher::session();
    let sink = playing.sink();
    tokio::spawn(async move {
        if let Err(error) = watcher.watch(sink).await {
            tracing::debug!(
                %error,
                "MPRIS watch ended; probe uses the normal stale threshold"
            );
        }
    });
}

#[cfg(test)]
mod tests;
