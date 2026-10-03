//! `stillwatch probe` over D-Bus: subscribe to `ProbeSample`, start the
//! probe, render each sample, and stop the probe on the way out.

use std::future::Future;
use std::io::Write;
use std::path::Path;
use std::pin::pin;
use std::time::Duration;

use anyhow::{Context as _, anyhow};
use futures_util::StreamExt as _;
use stillwatch_core::config::Config;
use stillwatch_ipc::json::{from_json, to_json};
use stillwatch_ipc::probe::ProbeSample;
use stillwatch_ipc::proxy::{ProbeSampleStream, StillwatchProxy};
use stillwatch_ipc::{config_file, paths};
use zbus::proxy::OwnerChangedStream;

use crate::args::ProbeArgs;
use crate::connect::DaemonError;
use crate::render::{self, Style, probe::CLEAR};

/// Streams probe samples until `stop` completes or `--count` samples have
/// arrived, then stops the probe.
///
/// Without `--interval`, the interval is `stale.check_interval_seconds` from
/// the config file (or its default if the file can't be read).
///
/// # Errors
///
/// Fails if the daemon refuses the interval, isn't running, stops while
/// probing, or sends a sample this CLI can't read, or if `out` can't be
/// written.
pub async fn run(
    proxy: &StillwatchProxy<'_>,
    args: &ProbeArgs,
    style: &Style,
    out: &mut dyn Write,
    stop: impl Future<Output = ()>,
) -> anyhow::Result<()> {
    let interval = match args.interval {
        Some(interval) => interval,
        None => configured_interval(&paths::config_file()?),
    };
    let interval_ms = u32::try_from(interval.as_millis())
        .map_err(|_| anyhow!("--interval can be at most {} ms", u32::MAX))?;
    // Subscribe first: the daemon sends the first sample as soon as the probe
    // starts.
    let samples = proxy
        .receive_probe_sample()
        .await
        .map_err(DaemonError::from)?;
    let owner = proxy
        .inner()
        .receive_owner_changed()
        .await
        .map_err(DaemonError::from)?;
    proxy
        .start_probe(interval_ms)
        .await
        .map_err(DaemonError::from)?;
    let streamed = stream(Streams { samples, owner }, args, style, out, stop).await;
    if streamed.is_ok() {
        proxy.stop_probe().await.map_err(DaemonError::from)?;
    }
    streamed
}

struct Streams<'p> {
    samples: ProbeSampleStream,
    owner: OwnerChangedStream<'p>,
}

async fn stream(
    mut streams: Streams<'_>,
    args: &ProbeArgs,
    style: &Style,
    out: &mut dyn Write,
    stop: impl Future<Output = ()>,
) -> anyhow::Result<()> {
    let mut stop = pin!(stop);
    let mut seen = 0;
    loop {
        tokio::select! {
            () = &mut stop => return Ok(()),
            owner = streams.owner.next() => {
                if !matches!(owner, Some(Some(_))) {
                    return Err(DaemonError::Stopped.into());
                }
            }
            signal = streams.samples.next() => {
                let signal = signal.ok_or(DaemonError::Stopped)?;
                let args_json = signal.args().map_err(DaemonError::from)?;
                let sample: ProbeSample =
                    from_json(args_json.json()).map_err(DaemonError::from)?;
                write_sample(&sample, args.json, style, out)?;
                seen += 1;
                if args.count.is_some_and(|count| seen >= count.get()) {
                    return Ok(());
                }
            }
        }
    }
}

/// Writes one sample as JSON or through [`render::probe`].
///
/// # Errors
///
/// Encoding the sample or writing to `out` fails.
pub(crate) fn write_sample(
    sample: &ProbeSample,
    json: bool,
    style: &Style,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    if json {
        writeln!(out, "{}", to_json(sample)?)?;
    } else {
        if style.redraw {
            out.write_all(CLEAR.as_bytes())?;
        }
        out.write_all(render::probe::render(sample, style).as_bytes())?;
    }
    out.flush().context("can't write the probe sample")
}

/// `stale.check_interval_seconds` from the config at `path`, falling back to
/// the default when the file is missing or invalid.
fn configured_interval(path: &Path) -> Duration {
    let stale = match config_file::load(path) {
        Ok(outcome) => outcome.config.stale,
        Err(err) => {
            tracing::debug!(%err, "probing at the default interval");
            Config::default().stale
        }
    };
    Duration::from_secs(u64::from(stale.check_interval_seconds))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use jiff::Timestamp;
    use jiff::tz::TimeZone;
    use stillwatch_core::stats::{Threshold, ThresholdReason};

    use super::*;

    fn sample() -> ProbeSample {
        ProbeSample {
            at: Timestamp::UNIX_EPOCH,
            threshold: Threshold::new(70, ThresholdReason::Normal),
            stale: false,
            outputs: Vec::new(),
        }
    }

    #[test]
    fn interval_comes_from_the_config_or_its_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let default = u64::from(Config::default().stale.check_interval_seconds);
        assert_eq!(configured_interval(&path), Duration::from_secs(default));
        fs::write(&path, "[stale]\ncheck_interval_seconds = 5\n").unwrap();
        assert_eq!(configured_interval(&path), Duration::from_secs(5));
        fs::write(&path, "[stale]\ncheck_interval_seconds = \"soon\"\n").unwrap();
        assert_eq!(configured_interval(&path), Duration::from_secs(default));
    }

    #[test]
    fn redraw_clears_before_each_text_sample() {
        let mut style = Style::plain(TimeZone::UTC);
        let mut out = Vec::new();
        write_sample(&sample(), false, &style, &mut out).unwrap();
        let plain = String::from_utf8(out).unwrap();
        assert!(plain.starts_with("1970-01-01 00:00:00  screen: not stale\n"));

        style.redraw = true;
        let mut out = Vec::new();
        write_sample(&sample(), false, &style, &mut out).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), format!("{CLEAR}{plain}"));

        let mut out = Vec::new();
        write_sample(&sample(), true, &style, &mut out).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            format!("{}\n", to_json(&sample()).unwrap())
        );
    }
}
