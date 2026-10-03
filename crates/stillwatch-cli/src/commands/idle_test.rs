//! `stillwatch idle-test`: the daemon's idle and gamepad sources and the
//! activity aggregator, run locally without the daemon.

use std::future::Future;
use std::io::{self, Write};
use std::pin::pin;
use std::sync::Arc;

use anyhow::Context as _;
use jiff::Timestamp;
use jiff::tz::TimeZone;
use stillwatch_core::activity::{ActivityOutput, ActivitySettings, Presence, WakeSource};
use stillwatch_core::backend::GamepadSource;
use stillwatch_core::backoff::Backoff;
use stillwatch_core::config::{Config, ConfigError};
use stillwatch_core::time::Clock;
use stillwatch_ipc::config_file::{self, ConfigFileError};
use stillwatchd::activity::{self, ActivitySources};
use stillwatchd::clock::TokioClock;
use stillwatchd::gamepad::{EvdevGamepadSource, GamepadSettings};
use stillwatchd::idle::WaylandIdleSource;
use stillwatchd::signals::{self, Signals};
use tokio::sync::{mpsc, watch as watch_channel};

use crate::args::IdleTestArgs;

/// Runs the sources from the config (with `--timeout` overriding
/// `idle.input_idle_minutes`) and prints each transition until Ctrl-C.
///
/// # Errors
///
/// Fails if the config can't be loaded, the runtime or signal handlers can't
/// start, stdout goes away, or the compositor can't do input idle.
pub fn run(args: &IdleTestArgs) -> anyhow::Result<()> {
    let config = load_config()?;
    let mut settings = ActivitySettings::from(&config);
    if let Some(timeout) = args.timeout {
        settings.input_idle = timeout;
    }
    let runtime = tokio::runtime::Runtime::new().context("can't start the async runtime")?;
    runtime.block_on(async {
        let mut signals = Signals::install().context("can't install signal handlers")?;
        let idle = WaylandIdleSource::new();
        let pads = config.activity.gamepad.then(|| {
            let pad_settings = GamepadSettings::from(&config.activity);
            EvdevGamepadSource::with_clock(&pad_settings, Arc::new(TokioClock))
        });
        let sources = ActivitySources {
            idle: &idle,
            gamepad: pads.as_ref().map(|pads| pads as &dyn GamepadSource),
        };
        let shutdown = async move {
            signals::wait_for_shutdown(&mut signals).await;
        };
        watch(
            sources,
            settings,
            &TokioClock,
            &mut io::stdout().lock(),
            shutdown,
        )
        .await
    })
}

fn load_config() -> anyhow::Result<Config> {
    match config_file::load_default() {
        Ok(outcome) => Ok(outcome.config),
        Err(ConfigFileError::Config(ConfigError::NotFound { .. })) => Ok(Config::default()),
        Err(error) => Err(error).context("can't load the config"),
    }
}

/// Runs [`activity::run`] over `sources` and writes a timestamped line to
/// `out` for every change of the combined idle state, until `shutdown`
/// completes.
///
/// # Errors
///
/// Fails if writing to `out` fails, or with the idle source's error if it
/// stops for good.
pub async fn watch(
    sources: ActivitySources<'_>,
    settings: ActivitySettings,
    clock: &dyn Clock,
    out: &mut impl Write,
    shutdown: impl Future<Output = ()>,
) -> anyhow::Result<()> {
    let inputs = if sources.gamepad.is_some() {
        "keyboard, mouse, and gamepads"
    } else {
        "keyboard and mouse (gamepads are off)"
    };
    writeln!(
        out,
        "Watching {inputs} with a {} idle timeout. Ctrl-C to stop.",
        humantime::format_duration(settings.input_idle)
    )?;

    let (changes_tx, mut changes) = mpsc::unbounded_channel();
    let (_, settings) = watch_channel::channel(settings);
    let running = activity::run(sources, settings, Backoff::default(), clock, |output| {
        if let ActivityOutput::Changed(presence) = output {
            let _ = changes_tx.send((clock.wall_now(), presence));
        }
    });
    let mut running = pin!(running);
    let mut shutdown = pin!(shutdown);
    loop {
        tokio::select! {
            result = &mut running => {
                result.context("input idle stopped")?;
                return Ok(());
            }
            () = &mut shutdown => return Ok(()),
            Some((at, presence)) = changes.recv() => {
                writeln!(out, "{}", line(at, &presence, sources.gamepad))?;
            }
        }
    }
}

/// One transition, as printed: local time, then what happened.
fn line(at: Timestamp, presence: &Presence, pads: Option<&dyn GamepadSource>) -> String {
    let what = match presence {
        Presence::Idle => "idle".to_owned(),
        Presence::Active(WakeSource::KeyboardMouse) => "active (keyboard/mouse)".to_owned(),
        Presence::Active(WakeSource::Gamepad { device }) => {
            format!("active (gamepad: {})", pad_name(pads, device))
        }
        Presence::Active(WakeSource::WatchRestarted) => "active (idle watch restarted)".to_owned(),
    };
    let local = at.to_zoned(TimeZone::system());
    format!("{}  {what}", local.strftime("%Y-%m-%d %H:%M:%S"))
}

/// The pad's reported name, or its id if it's already gone.
fn pad_name(pads: Option<&dyn GamepadSource>, id: &str) -> String {
    pads.and_then(|pads| pads.devices().into_iter().find(|pad| pad.id == id))
        .map_or_else(|| id.to_owned(), |pad| pad.name)
}

#[cfg(test)]
mod tests;
