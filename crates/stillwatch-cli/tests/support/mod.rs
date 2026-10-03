//! A fake daemon on a private session bus, and ways to point the CLI at it.
//! Nothing here touches the user's session bus.

#![allow(dead_code)]

use std::future;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser as _;
use jiff::tz::TimeZone;
use stillwatch_cli::Cli;
use stillwatch_cli::commands::daemon::execute;
use stillwatch_cli::commands::{Route, route};
use stillwatch_cli::render::Style;
use stillwatch_testkit::PrivateBus;
use stillwatchd::service::Service;
use stillwatchd::service::fake::FakeHandle;

/// How long any single wait in a test may take.
pub const WAIT: Duration = Duration::from_secs(5);

pub type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

/// A [`FakeHandle`] served under the daemon's well-known name.
pub struct Daemon {
    pub fake: Arc<FakeHandle>,
    pub service: Service,
    pub bus: PrivateBus,
}

impl Daemon {
    /// Starts a private bus with a fake daemon on it, or `None` when
    /// `dbus-daemon` isn't installed (and isn't required).
    pub async fn start() -> TestResult<Option<Self>> {
        let Some(bus) = PrivateBus::start()? else {
            return Ok(None);
        };
        let fake = Arc::new(FakeHandle::new());
        let conn = bus.connect().await?;
        let service = Service::claim(conn, Arc::clone(&fake) as _).await?;
        Ok(Some(Self { fake, service, bus }))
    }

    pub fn address(&self) -> &str {
        self.bus.address()
    }
}

/// What a command printed and how it ended.
pub struct Ran {
    pub result: anyhow::Result<()>,
    pub out: String,
}

impl Ran {
    /// The error message, or an empty string if the command succeeded.
    pub fn error(&self) -> String {
        self.result
            .as_ref()
            .err()
            .map(|err| format!("{err:#}"))
            .unwrap_or_default()
    }

    /// The exit code the binary would use.
    pub fn exit_code(&self) -> u8 {
        match &self.result {
            Ok(()) => stillwatch_cli::exit::OK,
            Err(err) => stillwatch_cli::exit::code(err),
        }
    }
}

/// Runs a daemon command in-process against the bus at `address`, with
/// plain UTC output. A probe runs until its `--count` or `stop`.
pub async fn stillwatch_until(
    address: &str,
    args: &[&str],
    stop: impl Future<Output = ()>,
) -> TestResult<Ran> {
    let cli = Cli::try_parse_from(std::iter::once("stillwatch").chain(args.iter().copied()))?;
    let Route::Daemon(request) = route(&cli.command) else {
        return Err(format!("{args:?} isn't a daemon command").into());
    };
    let mut out = Vec::new();
    let style = Style::plain(TimeZone::UTC);
    let result = execute(request, Some(address), &style, &mut out, stop).await;
    Ok(Ran {
        result,
        out: String::from_utf8(out)?,
    })
}

/// [`stillwatch_until`] with nothing to stop it early.
pub async fn stillwatch(address: &str, args: &[&str]) -> TestResult<Ran> {
    stillwatch_until(address, args, future::pending()).await
}
