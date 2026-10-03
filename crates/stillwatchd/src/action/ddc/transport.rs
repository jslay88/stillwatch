//! The seam between the blanker and the i2c buses.

use super::DdcError;

/// A display that answered on a DDC bus, with the EDID it reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DdcDisplay {
    /// The transport's handle for the display, for example `i2c-3`.
    pub id: String,
    /// Raw EDID, at least the base block.
    pub edid: Vec<u8>,
}

/// What one scan of the buses found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Scan {
    /// Displays that returned an EDID.
    pub displays: Vec<DdcDisplay>,
    /// Device nodes that refused to open with a permission error.
    pub denied: Vec<String>,
}

/// Blocking DDC/CI access. Every call may take hundreds of milliseconds, so
/// callers run them on the blocking pool.
pub(crate) trait DdcTransport: Send + Sync + 'static {
    /// Finds the displays reachable over DDC.
    fn scan(&self) -> Result<Scan, DdcError>;

    /// Reads VCP feature `code`'s current value from `display`.
    fn get_vcp(&self, display: &str, code: u8) -> Result<u16, String>;

    /// Sets VCP feature `code` on `display`.
    fn set_vcp(&self, display: &str, code: u8, value: u16) -> Result<(), String>;
}
