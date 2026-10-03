//! Which i2c buses to probe: only those belonging to a GPU.
//!
//! Probing means reading address `0x50`, which on a motherboard system bus
//! is a RAM SPD EEPROM, not a monitor. Matching adapters to DRM devices is
//! exact where adapter name lists (what `ddcutil` skips) are guesses. It also
//! covers NVIDIA's driver, which doesn't link connectors to their bus.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Where the kernel lists i2c character devices.
pub(crate) const SYSFS_I2C_DEV: &str = "/sys/class/i2c-dev";

/// Bus names (`i2c-3`) under `i2c_dev` whose adapter sits below a DRM card's
/// device under `drm`, in bus number order.
pub(crate) fn gpu_buses(i2c_dev: &Path, drm: &Path) -> io::Result<Vec<String>> {
    let gpus = gpu_devices(drm)?;
    let mut buses: Vec<(u32, String)> = Vec::new();
    for entry in fs::read_dir(i2c_dev)? {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Some(number) = bus_number(&name) else {
            continue;
        };
        let Ok(adapter) = fs::canonicalize(entry.path().join("device")) else {
            continue;
        };
        if gpus.iter().any(|gpu| adapter.starts_with(gpu)) {
            buses.push((number, name));
        }
    }
    buses.sort();
    Ok(buses.into_iter().map(|(_, name)| name).collect())
}

/// The devices behind `card0`, `card1`, ... (not their connectors).
fn gpu_devices(drm: &Path) -> io::Result<Vec<PathBuf>> {
    let mut gpus = Vec::new();
    for entry in fs::read_dir(drm)? {
        let entry = entry?;
        let is_card = entry
            .file_name()
            .to_str()
            .and_then(|name| name.strip_prefix("card"))
            .is_some_and(|index| !index.is_empty() && index.bytes().all(|b| b.is_ascii_digit()));
        if is_card && let Ok(device) = fs::canonicalize(entry.path().join("device")) {
            gpus.push(device);
        }
    }
    Ok(gpus)
}

fn bus_number(name: &str) -> Option<u32> {
    name.strip_prefix("i2c-")?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::ddc::fixtures::SysfsTree as Tree;

    impl Tree {
        fn buses(&self) -> Vec<String> {
            gpu_buses(&self.path("i2c-dev"), &self.path("drm")).unwrap()
        }
    }

    #[test]
    fn keeps_only_gpu_buses_in_number_order() {
        let tree = Tree::new();
        tree.class_entry("drm", "card1", "pci/01:00.0");
        tree.class_entry("drm", "card0", "pci/79:00.0");
        tree.class_entry(
            "drm",
            "card1-HDMI-A-1",
            "pci/01:00.0/drm/card1/card1-HDMI-A-1",
        );
        tree.class_entry("drm", "renderD128", "pci/01:00.0/drm/renderD128");
        tree.class_entry("i2c-dev", "i2c-0", "platform/AMDI0010:00/i2c-0");
        tree.class_entry("i2c-dev", "i2c-3", "pci/01:00.0/i2c-3");
        tree.class_entry("i2c-dev", "i2c-12", "pci/79:00.0/i2c-12");
        tree.class_entry(
            "i2c-dev",
            "i2c-14",
            "pci/79:00.0/drm/card0/card0-DP-4/i2c-14",
        );
        tree.class_entry("i2c-dev", "i2c-2", "pci/01:00.0/i2c-2");
        tree.class_entry("i2c-dev", "i2c-7", "pci/14.0/i2c-7");
        fs::create_dir(tree.path("i2c-dev/i2c-99")).unwrap();
        fs::create_dir(tree.path("i2c-dev/bogus")).unwrap();

        assert_eq!(tree.buses(), ["i2c-2", "i2c-3", "i2c-12", "i2c-14"]);
    }

    #[test]
    fn no_gpu_means_no_buses() {
        let tree = Tree::new();
        tree.class_entry("i2c-dev", "i2c-7", "pci/14.0/i2c-7");
        fs::create_dir(tree.path("drm/card0")).unwrap();
        assert_eq!(tree.buses(), Vec::<String>::new());
    }

    #[test]
    fn missing_class_directories_are_errors() {
        let tree = Tree::new();
        let missing = tree.path("missing");
        assert!(gpu_buses(&missing, &tree.path("drm")).is_err());
        assert!(gpu_buses(&tree.path("i2c-dev"), &missing).is_err());
    }

    #[test]
    fn bus_numbers_parse_from_names() {
        assert_eq!(bus_number("i2c-3"), Some(3));
        assert_eq!(bus_number("i2c-"), None);
        assert_eq!(bus_number("i2c-x"), None);
        assert_eq!(bus_number("card0"), None);
    }
}
