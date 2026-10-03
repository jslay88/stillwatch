//! Whether `kscreen-doctor` and a GPU i2c node are usable.
//!
//! Neither is invoked for a side effect: `kscreen-doctor` is only looked up
//! on `PATH`, and an i2c node is opened and closed without a DDC transaction.

use std::fs::OpenOptions;
use std::path::Path;

use super::ProbeEnv;
use super::facts::ToolFacts;
use crate::action::ddc::gpu_buses;
use crate::action::dpms::PROGRAM;

/// Looks up the tools described by `env`.
#[must_use]
pub fn read(env: &ProbeEnv) -> ToolFacts {
    ToolFacts {
        kscreen_doctor: on_path(&env.path),
        i2c: i2c_accessible(&env.i2c_dev, &env.drm, &env.dev),
    }
}

fn on_path(path: &std::ffi::OsStr) -> bool {
    std::env::split_paths(path).any(|dir| dir.join(PROGRAM).is_file())
}

fn i2c_accessible(i2c_dev: &Path, drm: &Path, dev: &Path) -> bool {
    let Ok(buses) = gpu_buses(i2c_dev, drm) else {
        return false;
    };
    buses.iter().any(|bus| can_open(&dev.join(bus)))
}

fn can_open(node: &Path) -> bool {
    OpenOptions::new().read(true).write(true).open(node).is_ok()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::OpenOptionsExt as _;

    use super::*;

    #[test]
    fn kscreen_doctor_is_a_file_on_path() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!on_path(dir.path().as_os_str()));
        let program = dir.path().join(PROGRAM);
        fs::write(&program, "").unwrap();
        assert!(on_path(dir.path().as_os_str()));
        let _ = program;
    }

    #[test]
    fn missing_sysfs_is_not_i2c_access() {
        let missing = Path::new("/nonexistent/stillwatch-i2c");
        assert!(!i2c_accessible(missing, missing, missing));
    }

    #[test]
    fn an_unopenable_node_is_not_access() {
        let dir = tempfile::tempdir().unwrap();
        let node = dir.path().join("i2c-3");
        OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o000)
            .open(&node)
            .unwrap();
        assert!(!can_open(&node));
    }
}
