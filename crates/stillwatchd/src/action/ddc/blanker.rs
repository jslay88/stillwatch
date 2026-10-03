//! What the blanker remembers: which outputs it put into standby, on which
//! bus, and their last observed power state.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use stillwatch_core::backend::{BackendError, EventSink};
use stillwatch_core::event::Event;
use tokio::sync::Notify;
use tokio::time;

use super::drm::DrmConnectors;
use super::resolve::{self, Target};
use super::transport::DdcTransport;
use super::{DdcError, POWER_ON, STANDBY_POWER_MODE, Timing, VCP_POWER_MODE, worker};

/// An output this blanker put into standby.
#[derive(Debug, Clone)]
struct Blanked {
    /// The transport's display handle.
    display: String,
    /// Last observed power state; `None` until the first poll.
    on: Option<bool>,
}

pub(crate) struct Shared {
    transport: Arc<dyn DdcTransport>,
    drm: DrmConnectors,
    timing: Timing,
    blanked: Mutex<BTreeMap<String, Blanked>>,
    changed: Notify,
}

impl Shared {
    pub(crate) fn new(
        transport: Arc<dyn DdcTransport>,
        drm: DrmConnectors,
        timing: Timing,
    ) -> Self {
        Self {
            transport,
            drm,
            timing,
            blanked: Mutex::new(BTreeMap::new()),
            changed: Notify::new(),
        }
    }

    /// Puts every resolvable output into standby. Outputs that fail don't
    /// stop the others; the first failure is returned.
    pub(crate) async fn blank(&self, outputs: &[String]) -> Result<(), BackendError> {
        let mut failure = None;
        for Target { output, display } in self.resolve(outputs).await? {
            let written = match display {
                Ok(display) => self
                    .write(&output, &display, STANDBY_POWER_MODE)
                    .await
                    .map(|()| display),
                Err(error) => Err(error),
            };
            match written {
                Ok(bus) => {
                    tracing::info!(output, bus, "display put into DDC/CI standby");
                    self.lock().insert(
                        output,
                        Blanked {
                            display: bus,
                            on: None,
                        },
                    );
                }
                Err(error) => {
                    tracing::warn!(output, %error, "DDC/CI standby failed");
                    failure.get_or_insert(error);
                }
            }
        }
        self.changed.notify_one();
        failure.map_or(Ok(()), |error| Err(error.into()))
    }

    /// Wakes the listed outputs (all, if empty) that this blanker put into
    /// standby. Others are left alone.
    pub(crate) async fn unblank(&self, outputs: &[String]) -> Result<(), BackendError> {
        let mut failure = None;
        for (output, bus) in self.take(outputs) {
            match self.write(&output, &bus, POWER_ON).await {
                Ok(()) => tracing::info!(output, bus, "display woken over DDC/CI"),
                Err(error) => {
                    tracing::warn!(output, %error, "DDC/CI wake failed");
                    failure.get_or_insert(error);
                }
            }
        }
        failure.map_or(Ok(()), |error| Err(error.into()))
    }

    /// Polls Power Mode while anything is blanked and sleeps otherwise.
    /// Never ends on its own.
    pub(crate) async fn watch(&self, sink: Arc<dyn EventSink>) -> Result<(), BackendError> {
        loop {
            if self.nothing_blanked() {
                self.changed.notified().await;
                continue;
            }
            time::sleep(self.timing.poll_interval).await;
            self.poll(sink.as_ref()).await;
        }
    }

    async fn poll(&self, sink: &dyn EventSink) {
        for (output, display) in self.snapshot() {
            let on = match self.read(&output, &display).await {
                Ok(value) => {
                    let Some(on) = power_state(value) else {
                        tracing::debug!(output, value, "unknown DDC/CI power mode");
                        continue;
                    };
                    on
                }
                // Many displays stop answering DDC/CI in standby.
                Err(error) => {
                    tracing::debug!(output, %error, "no power mode reply while blanked, counting as off");
                    false
                }
            };
            if self.record(&output, on) {
                tracing::info!(output, on, "DDC/CI power mode changed");
                sink.send(Event::DisplayPower { output, on });
            }
        }
    }

    async fn resolve(&self, outputs: &[String]) -> Result<Vec<Target>, DdcError> {
        let transport = Arc::clone(&self.transport);
        let drm = self.drm.clone();
        let outputs = outputs.to_vec();
        worker::run("scan", self.timing.scan_timeout, move || {
            resolve::resolve(transport.as_ref(), &drm, &outputs)
        })
        .await
    }

    async fn write(&self, output: &str, display: &str, value: u16) -> Result<(), DdcError> {
        let (output, display) = (output.to_owned(), display.to_owned());
        self.call("write", move |bus| {
            bus.set_vcp(&display, VCP_POWER_MODE, value)
                .map_err(|detail| DdcError::WriteFailed {
                    output: output.clone(),
                    code: VCP_POWER_MODE,
                    value,
                    detail,
                })
        })
        .await
    }

    async fn read(&self, output: &str, display: &str) -> Result<u16, DdcError> {
        let (output, display) = (output.to_owned(), display.to_owned());
        self.call("read", move |bus| {
            bus.get_vcp(&display, VCP_POWER_MODE)
                .map_err(|detail| DdcError::ReadFailed {
                    output: output.clone(),
                    code: VCP_POWER_MODE,
                    detail,
                })
        })
        .await
    }

    async fn call<T, F>(&self, what: &str, op: F) -> Result<T, DdcError>
    where
        F: Fn(&dyn DdcTransport) -> Result<T, DdcError> + Clone + Send + 'static,
        T: Send + 'static,
    {
        let transport = Arc::clone(&self.transport);
        worker::retry(&self.timing, what, move || op(transport.as_ref())).await
    }

    fn take(&self, outputs: &[String]) -> Vec<(String, String)> {
        let mut blanked = self.lock();
        let taken: Vec<(String, Blanked)> = if outputs.is_empty() {
            std::mem::take(&mut *blanked).into_iter().collect()
        } else {
            outputs
                .iter()
                .filter_map(|output| blanked.remove_entry(output))
                .collect()
        };
        taken
            .into_iter()
            .map(|(output, entry)| (output, entry.display))
            .collect()
    }

    fn snapshot(&self) -> Vec<(String, String)> {
        self.lock()
            .iter()
            .map(|(output, entry)| (output.clone(), entry.display.clone()))
            .collect()
    }

    /// Stores `on` for a still-blanked output; true if it changed.
    fn record(&self, output: &str, on: bool) -> bool {
        match self.lock().get_mut(output) {
            Some(entry) if entry.on != Some(on) => {
                entry.on = Some(on);
                true
            }
            _ => false,
        }
    }

    fn nothing_blanked(&self) -> bool {
        self.lock().is_empty()
    }

    fn lock(&self) -> MutexGuard<'_, BTreeMap<String, Blanked>> {
        self.blanked.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Decodes a Power Mode reading: on, off (any of the MCCS DPM off states), or
/// `None` for values MCCS doesn't define.
pub(crate) fn power_state(value: u16) -> Option<bool> {
    match value & 0xFF {
        0x01 => Some(true),
        0x02..=0x05 => Some(false),
        _ => None,
    }
}
