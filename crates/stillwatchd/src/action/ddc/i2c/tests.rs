//! Without i2c hardware these cover discovery and error paths: the "device
//! nodes" are regular files, so the first i2c ioctl fails before any bus
//! traffic.

use std::fs;
use std::os::unix::fs::PermissionsExt as _;

use super::*;
use crate::action::ddc::drm::DrmConnectors;
use crate::action::ddc::fixtures::SysfsTree;
use crate::action::ddc::resolve;

/// A tree with one GPU (`card1`) owning `i2c-3` and `i2c-4`, plus a
/// motherboard bus `i2c-7`, and a transport over it.
fn gpu_tree() -> (SysfsTree, I2cTransport) {
    let tree = SysfsTree::new();
    tree.class_entry("drm", "card1", "pci/01:00.0");
    tree.class_entry("i2c-dev", "i2c-3", "pci/01:00.0/i2c-3");
    tree.class_entry("i2c-dev", "i2c-4", "pci/01:00.0/i2c-4");
    tree.class_entry("i2c-dev", "i2c-7", "pci/14.0/i2c-7");
    let transport =
        I2cTransport::with_roots(tree.path("i2c-dev"), tree.path("drm"), tree.path("dev"));
    (tree, transport)
}

fn node(tree: &SysfsTree, bus: &str, mode: u32) {
    let path = tree.path(&format!("dev/{bus}"));
    fs::write(&path, []).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
}

/// Root ignores file modes, so permission tests can't run as root.
fn mode_is_enforced(tree: &SysfsTree) -> bool {
    let probe = tree.path("probe");
    fs::write(&probe, []).unwrap();
    fs::set_permissions(&probe, fs::Permissions::from_mode(0o000)).unwrap();
    fs::File::open(&probe).is_err()
}

#[test]
fn no_gpu_buses_is_unavailable() {
    let tree = SysfsTree::new();
    let transport =
        I2cTransport::with_roots(tree.path("i2c-dev"), tree.path("drm"), tree.path("dev"));
    assert_eq!(
        transport.scan(),
        Err(DdcError::Unavailable(
            "no i2c bus belongs to a GPU; is the i2c-dev module loaded?".into()
        ))
    );
}

#[test]
fn unreadable_sysfs_is_unavailable() {
    let tree = SysfsTree::new();
    let transport =
        I2cTransport::with_roots(tree.path("missing"), tree.path("drm"), tree.path("dev"));
    let error = transport.scan().unwrap_err();
    assert!(
        error
            .to_string()
            .starts_with("DDC/CI unavailable: can't list i2c buses")
    );
}

#[test]
fn buses_without_an_edid_are_skipped() {
    let (tree, transport) = gpu_tree();
    node(&tree, "i2c-3", 0o600);
    assert_eq!(transport.scan(), Ok(Scan::default()));
}

#[test]
fn unopenable_nodes_are_reported_as_denied() {
    let (tree, transport) = gpu_tree();
    if !mode_is_enforced(&tree) {
        return;
    }
    node(&tree, "i2c-3", 0o000);
    node(&tree, "i2c-4", 0o000);
    node(&tree, "i2c-7", 0o000);
    let scan = transport.scan().unwrap();
    assert_eq!(scan.displays, Vec::new());
    assert_eq!(
        scan.denied,
        [
            tree.path("dev/i2c-3").display().to_string(),
            tree.path("dev/i2c-4").display().to_string(),
        ]
    );
}

#[test]
fn vcp_calls_report_open_and_bus_errors() {
    let (tree, transport) = gpu_tree();
    let missing = transport.get_vcp("i2c-3", 0xD6).unwrap_err();
    assert!(missing.starts_with("can't open"), "{missing}");

    node(&tree, "i2c-3", 0o600);
    assert!(transport.get_vcp("i2c-3", 0xD6).is_err());
    assert!(transport.set_vcp("i2c-3", 0xD6, 0x01).is_err());
}

/// Read-only check against real hardware: matches every connected output to
/// its bus and reads Power Mode. Never writes a VCP value. Run with
/// `cargo test -p stillwatchd -- --ignored reads_power_mode`.
#[test]
#[ignore = "needs a DDC/CI display and i2c access"]
fn reads_power_mode_of_connected_displays() {
    let transport = I2cTransport::system();
    let drm = DrmConnectors::new(SYSFS_DRM);
    let scan = transport.scan().unwrap();
    for display in &scan.displays {
        eprintln!(
            "{}: {:?}",
            display.id,
            crate::action::ddc::edid::parse(&display.edid)
        );
    }
    for connector in drm.connected().unwrap() {
        match resolve::find(&connector, &scan) {
            Ok(display) => {
                let mode = transport.get_vcp(&display.id, 0xD6);
                eprintln!("{} -> {}: 0xD6 = {mode:#04x?}", connector.name, display.id);
            }
            Err(error) => eprintln!("{}: {error}", connector.name),
        }
    }
}
