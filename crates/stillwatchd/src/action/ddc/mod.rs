//! `blank_method = "ddc_standby"`: the MCCS Power Mode command (VCP `0xD6`)
//! over DDC/CI.
//!
//! Unlike DPMS, the video signal stays up and the monitor itself goes into
//! standby, which lets panel compensation (Pixel Cleaning on the PG48UQ)
//! run. Each output is matched to its i2c bus by EDID identity, then:
//!
//! - **blank** writes [`STANDBY_POWER_MODE`] to `0xD6`,
//! - **unblank** writes [`POWER_ON`],
//! - **watch** reads `0xD6` every [`POLL_INTERVAL`] while anything is blanked
//!   (DDC/CI has no events) and reports changes as `Event::DisplayPower`.
//!
//! Only the standard Power Mode feature is ever written; vendor codes are
//! unsafe and undiscoverable here. Every bus call runs on the blocking pool
//! with a timeout and bounded retries.

mod blanker;
mod buses;
mod drm;
mod edid;
mod error;
mod i2c;
mod resolve;
mod transport;
mod worker;

#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod mock;

use std::sync::Arc;
use std::time::Duration;

use stillwatch_core::backend::{BackendFuture, Blanker, EventSink};

pub use error::DdcError;

use blanker::Shared;
use drm::{DrmConnectors, SYSFS_DRM};
use i2c::I2cTransport;
use transport::DdcTransport;

/// MCCS VCP feature code for Power Mode.
pub const VCP_POWER_MODE: u8 = 0xD6;

/// Power Mode value for on ("DPM: On, DPMS: Off").
pub const POWER_ON: u16 = 0x01;

/// The Power Mode value written to blank: MCCS "DPM: Off, DPMS: Off".
///
/// `0x05` (power off) acts like the power button, and many displays stop
/// answering DDC/CI after it, so only the button wakes them again. DPM off
/// keeps DDC/CI alive on most displays. If a display only reaches real
/// standby with another value, this is the one place to change it.
pub const STANDBY_POWER_MODE: u16 = 0x04;

/// How often Power Mode is read while blanked.
pub const POLL_INTERVAL: Duration = Duration::from_secs(10);

/// Limit for one VCP read or write. The DDC/CI spec allows a display 50 ms
/// to answer, plus retries inside the driver.
const IO_TIMEOUT: Duration = Duration::from_secs(3);

/// Limit for a whole bus scan, which reads an EDID from every GPU bus.
const SCAN_TIMEOUT: Duration = Duration::from_secs(15);

/// Tries per VCP read or write, including the first.
const ATTEMPTS: u32 = 3;

/// Pause between tries, above the spec's 50 ms minimum between commands.
const RETRY_DELAY: Duration = Duration::from_millis(100);

/// Time limits and retry policy for DDC/CI calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Timing {
    pub poll_interval: Duration,
    pub io_timeout: Duration,
    pub scan_timeout: Duration,
    pub attempts: u32,
    pub retry_delay: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            poll_interval: POLL_INTERVAL,
            io_timeout: IO_TIMEOUT,
            scan_timeout: SCAN_TIMEOUT,
            attempts: ATTEMPTS,
            retry_delay: RETRY_DELAY,
        }
    }
}

/// Blanks displays with DDC/CI standby.
///
/// Construction opens nothing; the buses are scanned on each `blank`. Errors
/// (no i2c access, no matching display, a failed write, a timeout) come back
/// as [`BackendError`](stillwatch_core::backend::BackendError)s built from
/// [`DdcError`] and never stop the daemon.
pub struct DdcBlanker {
    shared: Arc<Shared>,
}

impl DdcBlanker {
    /// A blanker over the system's GPU i2c buses and DRM connectors.
    #[must_use]
    pub fn new() -> Self {
        Self::with_parts(
            Arc::new(I2cTransport::system()),
            DrmConnectors::new(SYSFS_DRM),
            Timing::default(),
        )
    }

    pub(crate) fn with_parts(
        transport: Arc<dyn DdcTransport>,
        drm: DrmConnectors,
        timing: Timing,
    ) -> Self {
        Self {
            shared: Arc::new(Shared::new(transport, drm, timing)),
        }
    }
}

impl Default for DdcBlanker {
    fn default() -> Self {
        Self::new()
    }
}

impl Blanker for DdcBlanker {
    fn blank<'a>(&'a self, outputs: &'a [String]) -> BackendFuture<'a, ()> {
        Box::pin(self.shared.blank(outputs))
    }

    fn unblank<'a>(&'a self, outputs: &'a [String]) -> BackendFuture<'a, ()> {
        Box::pin(self.shared.unblank(outputs))
    }

    fn watch(&self, sink: Arc<dyn EventSink>) -> BackendFuture<'_, ()> {
        Box::pin(self.shared.watch(sink))
    }
}

#[cfg(test)]
mod tests;
