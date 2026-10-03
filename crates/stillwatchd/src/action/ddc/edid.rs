//! The identity fields of an EDID base block, used to tell which DDC/CI bus
//! drives which DRM connector.
//!
//! Both sides go through this one parser: the connector's EDID from sysfs and
//! the EDID each i2c bus reports at address `0x50`.

use std::fmt;

/// Length of the EDID base block, the only part identity comes from.
pub(crate) const BASE_BLOCK_LEN: usize = 128;

const HEADER: [u8; 8] = [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00];
const DESCRIPTOR_OFFSETS: [usize; 4] = [54, 72, 90, 108];
const DESCRIPTOR_LEN: usize = 18;
const DESCRIPTOR_TEXT: usize = 5;
const TAG_SERIAL: u8 = 0xFF;
const TAG_NAME: u8 = 0xFC;

/// Why EDID bytes couldn't be parsed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum EdidError {
    /// Fewer bytes than one base block.
    #[error("EDID is {0} bytes, shorter than the 128-byte base block")]
    TooShort(usize),
    /// The fixed 8-byte header is missing.
    #[error("EDID header is missing")]
    BadHeader,
    /// The manufacturer ID isn't three letters.
    #[error("EDID manufacturer ID {0:#06x} isn't three letters")]
    BadManufacturer(u16),
}

/// Who made a display and which unit it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EdidIdentity {
    /// Three-letter PNP manufacturer ID, for example `AUS`.
    pub manufacturer: String,
    /// Manufacturer's product code.
    pub product: u16,
    /// Numeric serial number. Often 0 or all ones when unset.
    pub serial: u32,
    /// The serial number descriptor, which many displays fill instead of
    /// the numeric one.
    pub serial_text: Option<String>,
    /// The product name descriptor. Only used in messages.
    pub name: Option<String>,
}

impl EdidIdentity {
    /// Whether both describe the same physical unit, as far as EDID can
    /// tell. Two units of one model with unset serials look the same.
    pub(crate) fn same_unit(&self, other: &Self) -> bool {
        self.manufacturer == other.manufacturer
            && self.product == other.product
            && self.serial == other.serial
            && self.serial_text == other.serial_text
    }
}

impl fmt::Display for EdidIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {:#06x}", self.manufacturer, self.product)?;
        if let Some(name) = &self.name {
            write!(f, " \"{name}\"")?;
        }
        write!(f, " serial {:#010x}", self.serial)?;
        if let Some(serial) = &self.serial_text {
            write!(f, " \"{serial}\"")?;
        }
        Ok(())
    }
}

/// The base block of `bytes`, if there is a whole one.
pub(crate) fn base_block(bytes: &[u8]) -> Option<&[u8]> {
    bytes.get(..BASE_BLOCK_LEN)
}

/// Parses the identity fields from EDID `bytes`. Extension blocks are
/// ignored and the checksum isn't checked, since both sides of a match read
/// the same EEPROM.
pub(crate) fn parse(bytes: &[u8]) -> Result<EdidIdentity, EdidError> {
    let block = base_block(bytes).ok_or(EdidError::TooShort(bytes.len()))?;
    if block[..HEADER.len()] != HEADER {
        return Err(EdidError::BadHeader);
    }
    let packed = u16::from_be_bytes([block[8], block[9]]);
    let mut identity = EdidIdentity {
        manufacturer: manufacturer(packed).ok_or(EdidError::BadManufacturer(packed))?,
        product: u16::from_le_bytes([block[10], block[11]]),
        serial: u32::from_le_bytes([block[12], block[13], block[14], block[15]]),
        serial_text: None,
        name: None,
    };
    for offset in DESCRIPTOR_OFFSETS {
        let descriptor = &block[offset..offset + DESCRIPTOR_LEN];
        if descriptor[..3] != [0, 0, 0] {
            continue;
        }
        let slot = match descriptor[3] {
            TAG_SERIAL => &mut identity.serial_text,
            TAG_NAME => &mut identity.name,
            _ => continue,
        };
        *slot = descriptor_text(&descriptor[DESCRIPTOR_TEXT..]);
    }
    Ok(identity)
}

/// Three 5-bit letters, `1` = `A`, packed big-endian with the top bit clear.
fn manufacturer(packed: u16) -> Option<String> {
    if packed & 0x8000 != 0 {
        return None;
    }
    [10, 5, 0]
        .into_iter()
        .map(|shift| {
            let letter = u32::from((packed >> shift) & 0x1F);
            (1..=26)
                .contains(&letter)
                .then(|| char::from_u32(u32::from(b'@') + letter))
                .flatten()
        })
        .collect()
}

/// Descriptor text ends at a line feed and is padded with spaces; some
/// displays leave it zeroed.
fn descriptor_text(bytes: &[u8]) -> Option<String> {
    let end = bytes
        .iter()
        .position(|&b| b == b'\n')
        .unwrap_or(bytes.len());
    let text: String = String::from_utf8_lossy(&bytes[..end])
        .trim_matches(|c: char| c.is_whitespace() || c.is_control())
        .to_owned();
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests;
