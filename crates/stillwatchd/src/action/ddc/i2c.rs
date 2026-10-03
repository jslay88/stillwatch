//! The real [`DdcTransport`]: Linux i2c-dev through `ddc-i2c`.
//!
//! `ddc-hi` would add a capabilities parser and the MCCS database on top of
//! this, neither of which is used here (the PG48UQ's capabilities string
//! fails anyway), and its YAML dependencies fail `cargo deny`. Opening
//! the nodes directly also surfaces `EACCES`, which the crates' enumerators
//! silently skip.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::io;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, PoisonError};

use ddc::{Ddc as _, Edid as _};
use ddc_i2c::I2cDeviceDdc;

use super::DdcError;
use super::buses::{SYSFS_I2C_DEV, gpu_buses};
use super::drm::SYSFS_DRM;
use super::edid::BASE_BLOCK_LEN;
use super::transport::{DdcDisplay, DdcTransport, Scan};

/// Base block plus the first extension, like `ddcutil` reads.
const EDID_READ_LEN: usize = 256;

/// DDC/CI over the i2c buses of the system's GPUs. Handles stay open between
/// calls; display handles are bus names like `i2c-3`.
pub(crate) struct I2cTransport {
    i2c_dev: PathBuf,
    drm: PathBuf,
    dev: PathBuf,
    open: Mutex<HashMap<String, I2cDeviceDdc>>,
}

impl I2cTransport {
    /// The running system's sysfs and `/dev`.
    pub(crate) fn system() -> Self {
        Self::with_roots(SYSFS_I2C_DEV, SYSFS_DRM, "/dev")
    }

    /// Looks for buses under `i2c_dev` belonging to cards under `drm`, and
    /// opens their nodes under `dev`.
    pub(crate) fn with_roots(
        i2c_dev: impl Into<PathBuf>,
        drm: impl Into<PathBuf>,
        dev: impl Into<PathBuf>,
    ) -> Self {
        Self {
            i2c_dev: i2c_dev.into(),
            drm: drm.into(),
            dev: dev.into(),
            open: Mutex::new(HashMap::new()),
        }
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<String, I2cDeviceDdc>> {
        self.open.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn with_handle<T>(
        &self,
        display: &str,
        op: impl FnOnce(&mut I2cDeviceDdc) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut open = self.lock();
        let handle = match open.entry(display.to_owned()) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                let node = self.dev.join(display);
                let handle = ddc_i2c::from_i2c_device(&node)
                    .map_err(|e| format!("can't open {}: {e}", node.display()))?;
                entry.insert(handle)
            }
        };
        op(handle)
    }
}

impl DdcTransport for I2cTransport {
    fn scan(&self) -> Result<Scan, DdcError> {
        let buses = gpu_buses(&self.i2c_dev, &self.drm)
            .map_err(|e| DdcError::Unavailable(format!("can't list i2c buses: {e}")))?;
        if buses.is_empty() {
            return Err(DdcError::Unavailable(
                "no i2c bus belongs to a GPU; is the i2c-dev module loaded?".into(),
            ));
        }
        let mut scan = Scan::default();
        let mut open = self.lock();
        open.clear();
        for bus in buses {
            let node = self.dev.join(&bus);
            let mut handle = match ddc_i2c::from_i2c_device(&node) {
                Ok(handle) => handle,
                Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
                    scan.denied.push(node.display().to_string());
                    continue;
                }
                Err(e) => {
                    tracing::debug!(node = %node.display(), error = %e, "can't open i2c bus");
                    continue;
                }
            };
            let mut edid = vec![0; EDID_READ_LEN];
            match handle.read_edid(0, &mut edid) {
                Ok(len) if len >= BASE_BLOCK_LEN => {
                    edid.truncate(len);
                    scan.displays.push(DdcDisplay {
                        id: bus.clone(),
                        edid,
                    });
                    open.insert(bus, handle);
                }
                Ok(len) => tracing::debug!(bus, len, "short EDID, skipping bus"),
                Err(e) => tracing::trace!(bus, error = %e, "no EDID on bus"),
            }
        }
        Ok(scan)
    }

    fn get_vcp(&self, display: &str, code: u8) -> Result<u16, String> {
        self.with_handle(display, |handle| {
            handle
                .get_vcp_feature(code)
                .map(|value| value.value())
                .map_err(|e| e.to_string())
        })
    }

    fn set_vcp(&self, display: &str, code: u8, value: u16) -> Result<(), String> {
        self.with_handle(display, |handle| {
            handle
                .set_vcp_feature(code, value)
                .map_err(|e| e.to_string())
        })
    }
}

#[cfg(test)]
mod tests;
