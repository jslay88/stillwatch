//! The `dpms` blank method on KDE Plasma: `kscreen-doctor --dpms` to switch
//! displays, and `org_kde_kwin_dpms_manager` to watch their power state.
//!
//! [`DpmsBlanker`] takes the outputs to switch as connector names; resolving
//! `action.outputs` (monitored or all) into that list is the caller's job, as
//! is recording history. An empty list switches every output.
//!
//! **`KWin` 6.7.5 applies DPMS to the whole workspace.** Its
//! `org_kde_kwin_dpms.set` ignores which output it was sent for, so a request
//! for any one output turns every output off (and any input wakes them all).
//! [`ActionRunner`](super::ActionRunner) does not send a partial target list
//! here. It blanks those outputs with the overlay instead. A direct `blank`
//! still excludes non-targets, which is correct for compositors that honor
//! it, but on `KWin` 6.7.5 they turn off too.
//!
//! [`watch`](Blanker::watch) reports `Event::DisplayPower` for each output:
//! its state once when first seen (on connect, or when it's plugged in), then
//! only on change. `standby` and `suspend` count as off. It doesn't report
//! outputs being added or removed: `Event::OutputsChanged` carries output
//! sizes, which come from capture and platform detection. A removed output's
//! state is forgotten, so it's reported afresh if it comes back.

mod kscreen;
mod power;
mod watch;

use std::sync::Arc;
use std::time::Duration;

use stillwatch_core::backend::{BackendError, BackendFuture, Blanker, EventSink};
use wayland_client::Connection;

pub use kscreen::{DEFAULT_TIMEOUT, PROGRAM};

use crate::outputs;
use crate::process::{CommandRunner, TokioRunner};
use crate::wayland::connect_to;
use kscreen::{Invocation, Power};

type Connector = dyn Fn() -> Result<Connection, BackendError> + Send + Sync;

/// A [`Blanker`] that switches DPMS with `kscreen-doctor` and watches it
/// through `org_kde_kwin_dpms_manager`.
///
/// `blank` and `unblank` with explicit outputs open a short Wayland
/// connection to list the connected outputs, so the others can be excluded.
/// `watch` holds its own connection; dropping the future closes it.
pub struct DpmsBlanker {
    runner: Arc<dyn CommandRunner>,
    connect: Box<Connector>,
    invocation: Invocation,
}

impl DpmsBlanker {
    /// Runs `kscreen-doctor` from `PATH` and connects using
    /// `WAYLAND_DISPLAY` / `WAYLAND_SOCKET`, like any client.
    #[must_use]
    pub fn new() -> Self {
        Self {
            runner: Arc::new(TokioRunner),
            connect: Box::new(|| connect_to(None)),
            invocation: Invocation::default(),
        }
    }

    /// Talks to Wayland display `display` (a socket name in
    /// `$XDG_RUNTIME_DIR`, or an absolute path) instead of the environment's,
    /// for both the watch and kscreen-doctor.
    #[must_use]
    pub fn on_display(mut self, display: impl Into<String>) -> Self {
        let display = display.into();
        self.invocation.display = Some(display.clone());
        self.connect = Box::new(move || connect_to(Some(&display)));
        self
    }

    /// Runs `program` instead of `kscreen-doctor` from `PATH`.
    #[must_use]
    pub fn with_program(mut self, program: impl Into<String>) -> Self {
        self.invocation.program = program.into();
        self
    }

    /// Kills kscreen-doctor after `timeout` instead of [`DEFAULT_TIMEOUT`].
    #[must_use]
    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.invocation.timeout = timeout;
        self
    }

    /// Runs commands through `runner` instead of spawning them directly.
    #[must_use]
    pub fn with_runner(mut self, runner: Arc<dyn CommandRunner>) -> Self {
        self.runner = runner;
        self
    }

    /// Connects with `connect` instead of the environment. kscreen-doctor
    /// still uses the environment's display (or [`on_display`](Self::on_display)'s).
    #[must_use]
    pub fn with_connector(
        mut self,
        connect: impl Fn() -> Result<Connection, BackendError> + Send + Sync + 'static,
    ) -> Self {
        self.connect = Box::new(connect);
        self
    }

    async fn switch(&self, power: Power, targets: &[String]) -> Result<(), BackendError> {
        let excluded = if targets.is_empty() {
            Vec::new()
        } else {
            let connected: Vec<String> = outputs::list_on(&(self.connect)()?)
                .await?
                .into_iter()
                .map(|output| output.name)
                .collect();
            let Some(excluded) = kscreen::exclusions(power, targets, &connected)? else {
                tracing::debug!(?targets, "no connected output to wake");
                return Ok(());
            };
            excluded
        };
        if !excluded.is_empty() {
            tracing::warn!(
                ?excluded,
                "KWin applies DPMS to every output, so excluded outputs may switch too"
            );
        }
        let spec = self.invocation.command(power, &excluded);
        tracing::info!(command = %spec, "switching DPMS");
        let output = self.runner.run(&spec).await?;
        kscreen::check(&output)
    }
}

impl Default for DpmsBlanker {
    fn default() -> Self {
        Self::new()
    }
}

impl Blanker for DpmsBlanker {
    fn blank<'a>(&'a self, outputs: &'a [String]) -> BackendFuture<'a, ()> {
        Box::pin(self.switch(Power::Off, outputs))
    }

    fn unblank<'a>(&'a self, outputs: &'a [String]) -> BackendFuture<'a, ()> {
        Box::pin(self.switch(Power::On, outputs))
    }

    fn watch(&self, sink: Arc<dyn EventSink>) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            let conn = (self.connect)()?;
            watch::watch_on(&conn, sink).await
        })
    }
}

#[cfg(test)]
mod tests;
