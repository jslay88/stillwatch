//! Locating the workspace and the tools the gates shell out to.

use std::env;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Root of the cargo workspace, i.e. the parent of the `xtask` crate.
pub fn root() -> Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .context("xtask crate has no parent directory")
}

/// Whether an executable named `tool` exists in any `PATH` directory.
pub fn on_path(tool: &str) -> bool {
    env::var_os("PATH").is_some_and(|path| in_dirs(tool, env::split_paths(&path)))
}

fn in_dirs(tool: &str, dirs: impl IntoIterator<Item = PathBuf>) -> bool {
    dirs.into_iter().any(|dir| is_executable(&dir.join(tool)))
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    path.metadata()
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use super::{in_dirs, root};

    #[test]
    fn root_contains_workspace_manifest() {
        assert!(root().unwrap().join("Cargo.toml").is_file());
    }

    #[test]
    fn finds_only_executable_files() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("tool");
        let plain = dir.path().join("plain");
        fs::write(&exe, "").unwrap();
        fs::write(&plain, "").unwrap();
        fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();

        let dirs = || vec![dir.path().join("missing"), dir.path().to_path_buf()];
        assert!(in_dirs("tool", dirs()));
        assert!(!in_dirs("plain", dirs()));
        assert!(!in_dirs("absent", dirs()));
    }
}
