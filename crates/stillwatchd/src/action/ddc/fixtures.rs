//! EDID fixtures and a fake sysfs DRM tree. `PG48UQ` is the real base block
//! read from Justin's ASUS ROG Swift PG48UQ on `HDMI-A-1`; the other EDIDs
//! patch its identity fields.

use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;

use tempfile::TempDir;

/// A sysfs-like tree: `devices/` holds the real directories, `drm/` and
/// `i2c-dev/` hold class entries whose `device` links point into it, and
/// `dev/` stands in for `/dev`.
pub(crate) struct SysfsTree(TempDir);

impl SysfsTree {
    pub(crate) fn new() -> Self {
        let tree = Self(tempfile::tempdir().unwrap());
        for dir in ["drm", "i2c-dev", "dev"] {
            fs::create_dir(tree.path(dir)).unwrap();
        }
        tree
    }

    pub(crate) fn path(&self, relative: &str) -> PathBuf {
        self.0.path().join(relative)
    }

    pub(crate) fn class_entry(&self, class: &str, name: &str, device: &str) {
        let dir = self.path(&format!("{class}/{name}"));
        fs::create_dir_all(&dir).unwrap();
        let target = self.path(&format!("devices/{device}"));
        fs::create_dir_all(&target).unwrap();
        symlink(target, dir.join("device")).unwrap();
    }
}

/// A sysfs DRM root holding `card1-<name>` for each connected connector.
pub(crate) fn drm_root(connected: &[(&str, &[u8])]) -> TempDir {
    let root = tempfile::tempdir().unwrap();
    for (name, edid) in connected {
        let dir = root.path().join(format!("card1-{name}"));
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("status"), "connected\n").unwrap();
        fs::write(dir.join("edid"), edid).unwrap();
    }
    root
}

/// The PG48UQ's EDID base block: `AUS`, product `0x48E0`, serial
/// `0xFFFFFFFF`, an empty serial descriptor, name `PG48UQ`.
pub(crate) const PG48UQ: [u8; 128] = [
    0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x06, 0xb3, 0xe0, 0x48, 0xff, 0xff, 0xff, 0xff,
    0x00, 0x00, 0x01, 0x03, 0x80, 0x3c, 0x22, 0x78, 0x3a, 0x8c, 0xe5, 0xad, 0x52, 0x44, 0xaf, 0x25,
    0x0c, 0x50, 0x54, 0x25, 0x4a, 0x00, 0x71, 0x4f, 0x81, 0xc0, 0x81, 0x40, 0x81, 0x80, 0xd1, 0xc0,
    0xd1, 0xfc, 0x95, 0x00, 0xb3, 0x00, 0x08, 0xe8, 0x00, 0x30, 0xf2, 0x70, 0x5a, 0x80, 0xb0, 0x58,
    0x8a, 0x00, 0xad, 0x11, 0x32, 0x00, 0x00, 0x1e, 0x00, 0x00, 0x00, 0xfd, 0x00, 0x30, 0x78, 0x12,
    0x12, 0x89, 0x00, 0x0a, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x00, 0x00, 0x00, 0xfc, 0x00, 0x50,
    0x47, 0x34, 0x38, 0x55, 0x51, 0x0a, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x00, 0x00, 0x00, 0xff,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0a, 0x02, 0xf3,
];

/// Offset of the PG48UQ's serial number descriptor text.
const SERIAL_TEXT: usize = 113;

/// The PG48UQ block with its numeric serial and serial descriptor replaced.
pub(crate) fn pg48uq_unit(serial: u32, serial_text: &str) -> Vec<u8> {
    let mut edid = PG48UQ.to_vec();
    edid[12..16].copy_from_slice(&serial.to_le_bytes());
    let mut text = [b' '; 13];
    text[..serial_text.len()].copy_from_slice(serial_text.as_bytes());
    if serial_text.len() < text.len() {
        text[serial_text.len()] = b'\n';
    }
    edid[SERIAL_TEXT..SERIAL_TEXT + 13].copy_from_slice(&text);
    edid
}

/// A different monitor: Dell (`DEL`), product `0xA0B1`, serial 42.
pub(crate) fn dell() -> Vec<u8> {
    let mut edid = PG48UQ.to_vec();
    edid[8..10].copy_from_slice(&0x10ACu16.to_be_bytes());
    edid[10..12].copy_from_slice(&0xA0B1u16.to_le_bytes());
    edid[12..16].copy_from_slice(&42u32.to_le_bytes());
    edid
}
