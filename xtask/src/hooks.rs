//! Installing the repository's git hooks.

use std::path::Path;

use anyhow::{Result, ensure};

use crate::process::Step;

/// Directory, relative to the workspace root, that holds the tracked hooks.
const HOOKS_DIR: &str = ".githooks";

/// The command that points git at the tracked hooks.
fn command() -> Step {
    Step::new("git", ["config", "core.hooksPath", HOOKS_DIR])
}

/// Sets `core.hooksPath` so git runs the hooks in `.githooks`.
pub fn install(root: &Path) -> Result<()> {
    let hook = root.join(HOOKS_DIR).join("pre-commit");
    ensure!(hook.is_file(), "{} is missing", hook.display());
    command().run(root)?;
    eprintln!("installed git hooks from {HOOKS_DIR}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{command, install};
    use crate::process::Step;

    #[test]
    fn sets_hooks_path() {
        assert_eq!(command().to_string(), "git config core.hooksPath .githooks");
    }

    #[test]
    fn installs_into_a_fresh_repository() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert!(install(root).is_err());

        Step::new("git", ["init", "--quiet"]).run(root).unwrap();
        std::fs::create_dir(root.join(".githooks")).unwrap();
        std::fs::write(root.join(".githooks/pre-commit"), "#!/bin/sh\n").unwrap();
        install(root).unwrap();

        let configured = Step::new("git", ["config", "core.hooksPath"])
            .output(root)
            .unwrap();
        assert_eq!(configured.trim(), ".githooks");
    }
}
