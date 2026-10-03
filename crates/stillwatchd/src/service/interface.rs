//! The `io.github.jslay88.Stillwatch1` object: argument checks and
//! translation onto the [`DaemonHandle`], nothing more.

// zbus generates signal emitter helpers without doc comments.
#![allow(missing_docs)]

use std::fmt::Display;
use std::sync::Arc;
use std::time::{Duration, Instant};

use jiff::{SignedDuration, Timestamp};
use stillwatch_core::event::ControlCommand;
use stillwatch_core::prompt::PromptOutcome;
use stillwatch_ipc::gamepad::GamepadInfo;
use stillwatch_ipc::json::{to_json, to_json_lines};
use stillwatch_ipc::player::PlayerInfo;
use stillwatch_ipc::probe::MIN_PROBE_INTERVAL_MS;
use stillwatch_ipc::prompt::outcome_from_answer;
use zbus::Connection;
use zbus::fdo::{Error, Result};
use zbus::message::Header;
use zbus::object_server::SignalEmitter;

use super::handle::DaemonHandle;
use super::probe::ProbeHub;

pub(super) struct Control {
    handle: Arc<dyn DaemonHandle>,
    probe: Arc<ProbeHub>,
}

impl Control {
    pub(super) const fn new(handle: Arc<dyn DaemonHandle>, probe: Arc<ProbeHub>) -> Self {
        Self { handle, probe }
    }

    async fn control(&self, command: ControlCommand) -> Result<()> {
        self.handle.control(command).await.map_err(failed)
    }

    fn checked_snooze(&self, duration: Duration) -> Result<Duration> {
        self.handle
            .validate_snooze(duration)
            .map_err(|err| Error::InvalidArgs(err.to_string()))
    }
}

#[zbus::interface(name = "io.github.jslay88.Stillwatch1")]
impl Control {
    async fn status(&self) -> Result<String> {
        let status = self.handle.status().await.map_err(failed)?;
        to_json(&status.into_payload()).map_err(failed)
    }

    async fn snooze(&self, seconds: u64) -> Result<()> {
        let duration = self.checked_snooze(Duration::from_secs(seconds))?;
        self.control(ControlCommand::Snooze(duration)).await
    }

    async fn cancel_snooze(&self) -> Result<()> {
        self.control(ControlCommand::CancelSnooze).await
    }

    async fn pause(&self) -> Result<()> {
        self.control(ControlCommand::Pause).await
    }

    async fn resume(&self) -> Result<()> {
        self.control(ControlCommand::Resume).await
    }

    async fn reload(&self) -> Result<(bool, Vec<String>)> {
        let report = self.handle.reload().await.map_err(failed)?;
        Ok((report.ok, report.errors))
    }

    async fn history(&self, since_seconds: u64) -> Result<String> {
        let since = history_since(Timestamp::now(), since_seconds);
        let entries = self.handle.history(since).await.map_err(failed)?;
        to_json_lines(&entries).map_err(failed)
    }

    async fn start_probe(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &Connection,
        interval_ms: u32,
    ) -> Result<()> {
        if interval_ms < MIN_PROBE_INTERVAL_MS {
            return Err(Error::InvalidArgs(format!(
                "probe interval must be at least {MIN_PROBE_INTERVAL_MS} ms"
            )));
        }
        let interval = Duration::from_millis(u64::from(interval_ms));
        let client = header.sender().map(ToString::to_string);
        self.probe
            .subscribe(connection, client, interval)
            .await
            .map_err(failed)
    }

    #[expect(
        clippy::needless_pass_by_value,
        reason = "zbus only passes the header by value"
    )]
    fn stop_probe(&self, #[zbus(header)] header: Header<'_>) {
        let client = header.sender().map(ToString::to_string);
        self.probe.unsubscribe(&client.unwrap_or_default());
    }

    async fn prompt_answer(&self, kind: &str, minutes: u32) -> Result<()> {
        let outcome = match outcome_from_answer(kind, minutes) {
            Ok(PromptOutcome::Snooze(duration)) => {
                PromptOutcome::Snooze(self.checked_snooze(duration)?)
            }
            Ok(outcome) => outcome,
            Err(err) => return Err(Error::InvalidArgs(err.to_string())),
        };
        self.handle.answer_prompt(outcome).await.map_err(failed)
    }

    async fn outputs(&self) -> Result<Vec<String>> {
        self.handle.outputs().await.map_err(failed)
    }

    fn gamepads(&self) -> Result<String> {
        let now = Instant::now();
        let pads: Vec<GamepadInfo> = self
            .handle
            .gamepads()
            .iter()
            .map(|device| GamepadInfo::from_device(device, now))
            .collect();
        to_json(&pads).map_err(failed)
    }

    async fn players(&self) -> Result<String> {
        let players = self.handle.players().await.map_err(failed)?;
        let listed: Vec<PlayerInfo> = players.iter().map(PlayerInfo::from).collect();
        to_json(&listed).map_err(failed)
    }

    #[zbus(signal)]
    pub(super) async fn state_changed(emitter: &SignalEmitter<'_>, state: &str)
    -> zbus::Result<()>;

    #[zbus(signal)]
    pub(super) async fn config_changed(
        emitter: &SignalEmitter<'_>,
        ok: bool,
        errors: &[String],
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    pub(super) async fn probe_sample(emitter: &SignalEmitter<'_>, json: &str) -> zbus::Result<()>;
}

/// The oldest history timestamp `History(since_seconds)` asks for: 0 means
/// everything, and a span reaching before the earliest timestamp clamps to it.
fn history_since(now: Timestamp, since_seconds: u64) -> Timestamp {
    if since_seconds == 0 {
        return Timestamp::MIN;
    }
    i64::try_from(since_seconds)
        .ok()
        .and_then(|secs| now.checked_sub(SignedDuration::from_secs(secs)).ok())
        .unwrap_or(Timestamp::MIN)
}

fn failed(err: impl Display) -> Error {
    Error::Failed(err.to_string())
}

#[cfg(test)]
mod tests {
    use stillwatch_core::backend::BackendError;

    use super::*;

    #[test]
    fn zero_since_means_all_history() {
        assert_eq!(history_since(Timestamp::now(), 0), Timestamp::MIN);
    }

    #[test]
    fn since_counts_back_from_now() {
        let now = Timestamp::from_second(1_790_000_000).unwrap();
        assert_eq!(
            history_since(now, 7200),
            Timestamp::from_second(1_790_000_000 - 7200).unwrap()
        );
    }

    #[test]
    fn since_before_the_earliest_timestamp_clamps() {
        let now = Timestamp::from_second(1_790_000_000).unwrap();
        assert_eq!(history_since(now, u64::MAX), Timestamp::MIN);
        assert_eq!(history_since(now, i64::MAX.unsigned_abs()), Timestamp::MIN);
    }

    #[test]
    fn failures_keep_their_message() {
        let err = failed(BackendError::Disconnected("daemon stopping".into()));
        assert_eq!(err, Error::Failed("disconnected: daemon stopping".into()));
        let err = failed(zbus::Error::NameTaken);
        assert!(matches!(err, Error::Failed(_)));
    }
}
