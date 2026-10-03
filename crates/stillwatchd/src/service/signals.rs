//! Emitting the service's signals from the rest of the daemon.

use stillwatch_core::state::State;
use stillwatch_ipc::OBJECT_PATH;
use stillwatch_ipc::json::to_json;
use stillwatch_ipc::probe::ProbeSample;
use zbus::Connection;
use zbus::object_server::SignalEmitter;

use super::ServiceError;
use super::handle::ReloadReport;
use super::interface::Control;

/// Emits `StateChanged`, `ConfigChanged`, and `ProbeSample` on the service's
/// object. Cheap to clone; every clone emits on the same connection.
#[derive(Debug, Clone)]
pub struct ServiceSignals {
    emitter: SignalEmitter<'static>,
}

impl ServiceSignals {
    /// Signals on `connection` from the service's object path.
    ///
    /// # Errors
    ///
    /// Fails only if the object path is invalid, which it isn't.
    pub fn new(connection: &Connection) -> Result<Self, zbus::Error> {
        Ok(Self {
            emitter: SignalEmitter::new(connection, OBJECT_PATH)?,
        })
    }

    /// The state machine moved to `state`.
    ///
    /// # Errors
    ///
    /// Fails if the signal can't be sent.
    pub async fn state_changed(&self, state: State) -> Result<(), ServiceError> {
        Ok(Control::state_changed(&self.emitter, state.as_str()).await?)
    }

    /// A config reload was attempted. Send it after every attempt, whatever
    /// triggered it.
    ///
    /// # Errors
    ///
    /// Fails if the signal can't be sent.
    pub async fn config_changed(&self, report: &ReloadReport) -> Result<(), ServiceError> {
        Ok(Control::config_changed(&self.emitter, report.ok, &report.errors).await?)
    }

    /// A probe sample, as JSON.
    ///
    /// # Errors
    ///
    /// Fails if the sample can't be encoded or the signal can't be sent.
    pub async fn probe_sample(&self, sample: &ProbeSample) -> Result<(), ServiceError> {
        let json = to_json(sample)?;
        Ok(Control::probe_sample(&self.emitter, &json).await?)
    }
}
