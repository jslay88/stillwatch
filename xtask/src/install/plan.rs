//! The files `install` writes and `uninstall` removes.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use walkdir::WalkDir;

use super::prefix::Prefix;
use super::rewrite::rewrite_bindir;

/// Printed after a successful install. Not run.
pub const ENABLE_HINT: &str =
    "systemctl --user daemon-reload && systemctl --user enable --now stillwatch";

/// Printed after uninstall. Not run.
pub const DISABLE_HINT: &str =
    "systemctl --user disable --now stillwatch && systemctl --user daemon-reload";

/// Release binaries, in install order.
const BINS: [&str; 3] = ["stillwatchd", "stillwatch", "stillwatch-gui"];

const UNIT: &str = "packaging/stillwatch.service";
const GUI_DESKTOP: &str = "packaging/io.github.jslay88.Stillwatch.desktop";
const TRAY_DESKTOP: &str = "packaging/io.github.jslay88.Stillwatch.Tray.desktop";
const DAEMON_DESKTOP: &str = "packaging/io.github.jslay88.Stillwatch.Daemon.desktop";

/// How a planned file is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Content {
    /// Copied byte for byte.
    Copy,
    /// Packaged `/usr/bin/<binary>` paths rewritten to this prefix.
    Bindir,
}

/// One file the install plan writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedFile {
    /// Path relative to the workspace root.
    pub source: PathBuf,
    /// Absolute destination.
    pub dest: PathBuf,
    /// Whether `Exec=` paths are rewritten.
    pub content: Content,
    /// `0o755` for binaries, `0o644` for the rest.
    pub executable: bool,
}

/// Everything install writes for `prefix`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallPlan {
    /// Files, in write order.
    pub files: Vec<PlannedFile>,
}

/// Builds the install plan for `prefix` from the packaging tree in `root`.
///
/// # Errors
///
/// Fails if the unit, a desktop file, or the icon tree is missing.
pub fn plan(root: &Path, prefix: &Prefix) -> Result<InstallPlan> {
    let mut files = Vec::new();
    for name in BINS {
        files.push(PlannedFile {
            source: PathBuf::from("target/release").join(name),
            dest: prefix.bin_dir().join(name),
            content: Content::Copy,
            executable: true,
        });
    }
    files.push(text(UNIT, prefix.unit_path(), Content::Bindir));
    files.push(text(
        GUI_DESKTOP,
        prefix.applications_dir().join(file_name(GUI_DESKTOP)?),
        Content::Copy,
    ));
    files.push(text(
        DAEMON_DESKTOP,
        prefix.applications_dir().join(file_name(DAEMON_DESKTOP)?),
        Content::Bindir,
    ));
    files.push(text(
        TRAY_DESKTOP,
        prefix.template_dir().join(file_name(TRAY_DESKTOP)?),
        Content::Copy,
    ));
    collect_icons(root, prefix, &mut files)?;
    for file in &files {
        if file.content == Content::Copy && file.source.starts_with("target/") {
            continue;
        }
        let source = root.join(&file.source);
        ensure!(source.is_file(), "missing {}", source.display());
    }
    Ok(InstallPlan { files })
}

/// Writes `plan` under `prefix`'s binary directory `bindir`.
///
/// # Errors
///
/// Fails if a source is missing or a destination can't be written.
pub fn apply(root: &Path, plan: &InstallPlan, bindir: &str) -> Result<()> {
    for file in &plan.files {
        let source = root.join(&file.source);
        if let Some(parent) = file.dest.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        match file.content {
            Content::Copy => {
                fs::copy(&source, &file.dest).with_context(|| {
                    format!(
                        "failed to copy {} to {}",
                        source.display(),
                        file.dest.display()
                    )
                })?;
            }
            Content::Bindir => {
                let text = fs::read_to_string(&source)
                    .with_context(|| format!("failed to read {}", source.display()))?;
                fs::write(&file.dest, rewrite_bindir(&text, bindir))
                    .with_context(|| format!("failed to write {}", file.dest.display()))?;
            }
        }
        set_mode(&file.dest, if file.executable { 0o755 } else { 0o644 })?;
    }
    Ok(())
}

/// Removes the destinations in `plan`. Missing files are ignored.
///
/// # Errors
///
/// Fails if a destination exists and can't be removed.
pub fn remove(plan: &InstallPlan) -> Result<()> {
    for file in &plan.files {
        match fs::remove_file(&file.dest) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(err)
                    .with_context(|| format!("failed to remove {}", file.dest.display()));
            }
        }
    }
    Ok(())
}

fn text(source: &str, dest: PathBuf, content: Content) -> PlannedFile {
    PlannedFile {
        source: PathBuf::from(source),
        dest,
        content,
        executable: false,
    }
}

fn file_name(path: &str) -> Result<&str> {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .with_context(|| format!("{path} has no file name"))
}

fn collect_icons(root: &Path, prefix: &Prefix, files: &mut Vec<PlannedFile>) -> Result<()> {
    let base = root.join("packaging/icons");
    ensure!(base.is_dir(), "missing {}", base.display());
    let mut found = Vec::new();
    for entry in WalkDir::new(&base) {
        let entry = entry.with_context(|| format!("failed to read {}", base.display()))?;
        if entry.file_type().is_file() {
            found.push(entry.into_path());
        }
    }
    found.sort();
    ensure!(!found.is_empty(), "no icons under {}", base.display());
    for source in found {
        let relative = source
            .strip_prefix(&base)
            .with_context(|| format!("{} is not under {}", source.display(), base.display()))?;
        let source_rel = source
            .strip_prefix(root)
            .with_context(|| format!("{} is not under {}", source.display(), root.display()))?;
        files.push(PlannedFile {
            source: source_rel.to_path_buf(),
            dest: prefix.root().join("share/icons").join(relative),
            content: Content::Copy,
            executable: false,
        });
    }
    Ok(())
}

fn set_mode(path: &Path, mode: u32) -> Result<()> {
    let mut permissions = fs::metadata(path)
        .with_context(|| format!("failed to stat {}", path.display()))?
        .permissions();
    permissions.set_mode(mode);
    fs::set_permissions(path, permissions)
        .with_context(|| format!("failed to set permissions on {}", path.display()))?;
    Ok(())
}

/// The line install prints, including the enable hint.
#[must_use]
pub fn enable_message(prefix: &Prefix) -> String {
    format!(
        "installed Stillwatch under {}\n{ENABLE_HINT}",
        prefix.root().display()
    )
}

/// The line uninstall prints, including the disable hint.
#[must_use]
pub fn disable_message(prefix: &Prefix) -> String {
    format!(
        "removed Stillwatch from {}\n{DISABLE_HINT}",
        prefix.root().display()
    )
}

/// `cargo build` is not part of the plan. Install checks the binaries exist
/// after the build so a partial target directory fails before anything is written.
pub fn require_binaries(root: &Path) -> Result<()> {
    for name in BINS {
        let path = root.join("target/release").join(name);
        if !path.is_file() {
            bail!(
                "missing {} (cargo build --release did not produce it)",
                path.display()
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    use super::{
        Content, DISABLE_HINT, ENABLE_HINT, apply, disable_message, enable_message, plan, remove,
    };
    use crate::install::Prefix;
    use crate::workspace;

    #[test]
    fn hints_are_the_systemctl_lines_and_are_not_commands_we_run() {
        assert_eq!(
            ENABLE_HINT,
            "systemctl --user daemon-reload && systemctl --user enable --now stillwatch"
        );
        assert_eq!(
            DISABLE_HINT,
            "systemctl --user disable --now stillwatch && systemctl --user daemon-reload"
        );
        let prefix = Prefix::parse("/tmp/stillwatch-prefix", Path::new("/home")).unwrap();
        assert!(enable_message(&prefix).ends_with(ENABLE_HINT));
        assert!(disable_message(&prefix).ends_with(DISABLE_HINT));
    }

    #[test]
    fn real_tree_plan_uses_xdg_paths_and_rewrites_only_absolute_execs() {
        let root = workspace::root().unwrap();
        let prefix = Prefix::parse("/tmp/stillwatch-prefix", Path::new("/home")).unwrap();
        let plan = plan(&root, &prefix).unwrap();
        let dest = |suffix: &str| {
            plan.files
                .iter()
                .find(|file| file.dest.ends_with(suffix))
                .unwrap_or_else(|| panic!("missing {suffix}"))
        };
        assert_eq!(dest("bin/stillwatchd").content, Content::Copy);
        assert!(dest("bin/stillwatchd").executable);
        assert_eq!(dest("stillwatch.service").content, Content::Bindir);
        assert_eq!(
            dest("share/applications/io.github.jslay88.Stillwatch.desktop").content,
            Content::Copy
        );
        assert_eq!(
            dest("share/applications/io.github.jslay88.Stillwatch.Daemon.desktop").content,
            Content::Bindir
        );
        assert_eq!(
            dest("share/stillwatch/io.github.jslay88.Stillwatch.Tray.desktop").content,
            Content::Copy
        );
        assert!(plan.files.iter().any(|file| {
            file.dest
                .ends_with("scalable/apps/io.github.jslay88.Stillwatch.svg")
        }));
        assert!(plan.files.iter().any(|file| {
            file.dest
                .ends_with("scalable/status/io.github.jslay88.Stillwatch-down.svg")
        }));
        assert!(!plan.files.iter().any(|file| {
            file.dest.extension().is_some_and(|ext| ext == "service")
                && file.source.starts_with("packaging")
                && !file.source.ends_with("stillwatch.service")
        }));
    }

    #[test]
    fn system_prefix_plan_uses_lib_systemd() {
        let root = workspace::root().unwrap();
        let prefix = Prefix::parse("/usr", Path::new("/home")).unwrap();
        let plan = plan(&root, &prefix).unwrap();
        assert!(
            plan.files
                .iter()
                .any(|file| { file.dest == Path::new("/usr/lib/systemd/user/stillwatch.service") })
        );
        assert!(
            plan.files
                .iter()
                .any(|file| file.dest == Path::new("/usr/bin/stillwatchd"))
        );
    }

    #[test]
    fn apply_rewrites_exec_and_uninstall_removes_it() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_fixture(root);
        let prefix_root = dir.path().join("prefix");
        let prefix = Prefix::parse(prefix_root.to_str().unwrap(), Path::new("/home")).unwrap();
        let plan = plan(root, &prefix).unwrap();
        let bindir = prefix.bin_dir_string().unwrap();
        apply(root, &plan, &bindir).unwrap();

        let unit = fs::read_to_string(prefix.unit_path()).unwrap();
        assert!(
            unit.contains(&format!("ExecStart={bindir}/stillwatchd\n")),
            "{unit}"
        );
        assert!(
            unit.contains("ExecReload=/usr/bin/kill -HUP $MAINPID\n"),
            "{unit}"
        );
        let daemon = fs::read_to_string(
            prefix
                .applications_dir()
                .join("io.github.jslay88.Stillwatch.Daemon.desktop"),
        )
        .unwrap();
        assert!(
            daemon.contains(&format!("Exec={bindir}/stillwatchd\n")),
            "{daemon}"
        );
        let gui = fs::read_to_string(
            prefix
                .applications_dir()
                .join("io.github.jslay88.Stillwatch.desktop"),
        )
        .unwrap();
        assert!(gui.contains("Exec=stillwatch-gui settings\n"), "{gui}");
        let mode = fs::metadata(prefix.bin_dir().join("stillwatchd"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o755);
        assert!(
            prefix
                .root()
                .join("share/icons/hicolor/scalable/apps/io.github.jslay88.Stillwatch.svg")
                .is_file()
        );

        remove(&plan).unwrap();
        assert!(!prefix.unit_path().exists());
        assert!(!prefix.bin_dir().join("stillwatchd").exists());
        remove(&plan).unwrap();
    }

    fn write_fixture(root: &Path) {
        fs::create_dir_all(root.join("packaging/icons/hicolor/scalable/apps")).unwrap();
        fs::create_dir_all(root.join("target/release")).unwrap();
        fs::write(
            root.join("packaging/stillwatch.service"),
            "ExecStart=/usr/bin/stillwatchd\nExecReload=/usr/bin/kill -HUP $MAINPID\n",
        )
        .unwrap();
        fs::write(
            root.join("packaging/io.github.jslay88.Stillwatch.desktop"),
            "Exec=stillwatch-gui settings\n",
        )
        .unwrap();
        fs::write(
            root.join("packaging/io.github.jslay88.Stillwatch.Tray.desktop"),
            "Exec=stillwatch-gui\n",
        )
        .unwrap();
        fs::write(
            root.join("packaging/io.github.jslay88.Stillwatch.Daemon.desktop"),
            "Exec=/usr/bin/stillwatchd\n",
        )
        .unwrap();
        fs::write(
            root.join("packaging/icons/hicolor/scalable/apps/io.github.jslay88.Stillwatch.svg"),
            "<svg/>\n",
        )
        .unwrap();
        for name in ["stillwatchd", "stillwatch", "stillwatch-gui"] {
            fs::write(root.join("target/release").join(name), name).unwrap();
        }
    }
}
