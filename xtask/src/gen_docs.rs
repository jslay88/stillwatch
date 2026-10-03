//! `cargo xtask gen-docs`: regenerates `docs/config.md` from the settings schema.

use std::fs;
use std::path::Path;

use anyhow::{Context as _, Result, bail};
use stillwatch_core::schema::markdown_reference;

/// The config reference, relative to the repo root.
pub const CONFIG_DOCS: &str = "docs/config.md";

/// What [`sync`] did.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The file already matched.
    UpToDate,
    /// The file was missing or stale and has been rewritten.
    Written,
}

/// Regenerates the config reference under `root`, or with `check` fails if
/// it's out of date instead of writing it.
pub fn run(root: &Path, check: bool) -> Result<()> {
    let generated = markdown_reference()?;
    match sync(&root.join(CONFIG_DOCS), &generated, check)? {
        Outcome::UpToDate => eprintln!("{CONFIG_DOCS} is up to date"),
        Outcome::Written => eprintln!("wrote {CONFIG_DOCS}"),
    }
    Ok(())
}

fn sync(path: &Path, generated: &str, check: bool) -> Result<Outcome> {
    if fs::read_to_string(path).is_ok_and(|current| current == generated) {
        return Ok(Outcome::UpToDate);
    }
    if check {
        bail!(
            "{} is out of date; run `cargo xtask gen-docs`",
            path.display()
        );
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    fs::write(path, generated).with_context(|| format!("writing {}", path.display()))?;
    Ok(Outcome::Written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_a_missing_file_then_reports_up_to_date() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_DOCS);
        assert_eq!(sync(&path, "docs\n", false).unwrap(), Outcome::Written);
        assert_eq!(fs::read_to_string(&path).unwrap(), "docs\n");
        assert_eq!(sync(&path, "docs\n", true).unwrap(), Outcome::UpToDate);
    }

    #[test]
    fn check_fails_on_a_stale_file_without_touching_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.md");
        fs::write(&path, "old\n").unwrap();
        let err = sync(&path, "new\n", true).unwrap_err().to_string();
        assert!(
            err.ends_with("is out of date; run `cargo xtask gen-docs`"),
            "{err}"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "old\n");
    }

    #[test]
    fn write_failures_name_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("file");
        fs::write(&file, "").unwrap();
        let err = sync(&file.join("config.md"), "new\n", false).unwrap_err();
        assert!(err.to_string().starts_with("creating "), "{err}");
    }

    #[test]
    fn run_checks_the_committed_docs() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        run(root, true).unwrap();
    }
}
