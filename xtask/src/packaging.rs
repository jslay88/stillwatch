//! Checks the systemd user unit and the desktop files.
//!
//! [`Gate::Packaging`](crate::gate::Gate::Packaging) lists `systemd-analyze`
//! and `desktop-file-validate`. A normal run skips the check when either tool
//! is missing. `--strict` fails instead, same as the other gates.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};

use crate::process::Step;

const UNIT: &str = "packaging/stillwatch.service";
const STAND_INS: [&str; 2] = ["/usr/bin/true", "/bin/true"];

/// Runs `systemd-analyze verify --user` on the unit and `desktop-file-validate`
/// on every `packaging/*.desktop`.
///
/// `ExecStart` is pointed at `/usr/bin/true` (or `/bin/true`) for the verify
/// step when the packaged binary is not installed yet. The file on disk is
/// not modified. The rest of the unit, including `ExecReload`, is what
/// systemd checks.
///
/// # Errors
///
/// Fails if a checker is missing, a packaging file can't be read, or a checker
/// rejects a file.
pub fn check(root: &Path) -> Result<()> {
    verify_unit(root)?;
    validate_desktops(root)?;
    Ok(())
}

fn verify_unit(root: &Path) -> Result<()> {
    let source =
        fs::read_to_string(root.join(UNIT)).with_context(|| format!("failed to read {UNIT}"))?;
    let stand_in = stand_in_exec(Path::is_file)?;
    let rendered = with_runnable_exec_start(&source, Path::is_file, stand_in);
    let dir = TempDir::new()?;
    let path = dir.path().join("stillwatch.service");
    fs::write(&path, &rendered).with_context(|| format!("failed to write {}", path.display()))?;
    let path = path_str(&path)?;
    let user = analyze(root, &["verify", "--user", path])?;
    if user.ok {
        return Ok(());
    }
    if user_manager_unavailable(&user.stderr) {
        eprintln!("no systemd user manager; verifying the unit with the system manager");
        let system = analyze(root, &["verify", path])?;
        if system.ok {
            return Ok(());
        }
        bail!(
            "`systemd-analyze verify {path}` exited with {}:\n{}",
            system.status,
            system.stderr
        );
    }
    bail!(
        "`systemd-analyze verify --user {path}` exited with {}:\n{}",
        user.status,
        user.stderr
    );
}

struct Analyze {
    ok: bool,
    status: String,
    stderr: String,
}

fn analyze(root: &Path, args: &[&str]) -> Result<Analyze> {
    let step = Step::new("systemd-analyze", args.iter().copied());
    eprintln!("+ {step}");
    let output = step.captured(root)?;
    Ok(Analyze {
        ok: output.status.success(),
        status: output.status.to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

/// `verify --user` needs a running user manager. GitHub's runners have the
/// binary and no manager, which fails before the unit is read.
fn user_manager_unavailable(stderr: &str) -> bool {
    stderr.contains("Failed to initialize manager") || stderr.contains("Failed to connect to bus")
}

fn validate_desktops(root: &Path) -> Result<()> {
    let dir = root.join("packaging");
    let mut files = Vec::new();
    for entry in fs::read_dir(&dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let entry = entry.with_context(|| format!("failed to read {}", dir.display()))?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "desktop") {
            files.push(path);
        }
    }
    files.sort();
    if files.is_empty() {
        bail!("no desktop files in {}", dir.display());
    }
    let args: Vec<String> = files
        .iter()
        .map(|path| path.display().to_string())
        .collect();
    Step::new("desktop-file-validate", args).run(root)
}

/// Replaces an `ExecStart=` program that `runnable` rejects with `stand_in`.
#[must_use]
fn with_runnable_exec_start(
    text: &str,
    runnable: impl Fn(&Path) -> bool,
    stand_in: &str,
) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let ending = &line[body.len()..];
        match rewritten_exec_start(body, &runnable, stand_in) {
            Some(rewritten) => {
                out.push_str(&rewritten);
                out.push_str(ending);
            }
            None => out.push_str(line),
        }
    }
    out
}

fn rewritten_exec_start(
    body: &str,
    runnable: &impl Fn(&Path) -> bool,
    stand_in: &str,
) -> Option<String> {
    let rest = body.strip_prefix("ExecStart=")?;
    let program = rest.split_whitespace().next()?;
    if runnable(Path::new(program)) {
        return None;
    }
    let mut line = format!("ExecStart={stand_in}");
    if let Some(args) = rest.strip_prefix(program) {
        line.push_str(args);
    }
    Some(line)
}

fn stand_in_exec(exists: impl Fn(&Path) -> bool) -> Result<&'static str> {
    STAND_INS
        .into_iter()
        .find(|candidate| exists(Path::new(candidate)))
        .context("neither /usr/bin/true nor /bin/true exists, so the unit can't be verified")
}

fn path_str(path: &Path) -> Result<&str> {
    path.to_str()
        .with_context(|| format!("{} is not valid UTF-8", path.display()))
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        let path =
            std::env::temp_dir().join(format!("stillwatch-unit-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&path)
            .with_context(|| format!("failed to create {}", path.display()))?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _unused = fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{check, with_runnable_exec_start};
    use crate::workspace::{self, on_path};

    const UNIT: &str = "\
ExecStart=/usr/bin/stillwatchd
ExecReload=/usr/bin/kill -HUP $MAINPID
";

    #[test]
    fn missing_exec_start_is_replaced_for_verify() {
        let rendered = with_runnable_exec_start(
            UNIT,
            |path| path == Path::new("/usr/bin/kill"),
            "/usr/bin/true",
        );
        assert!(rendered.contains("ExecStart=/usr/bin/true\n"), "{rendered}");
        assert!(
            rendered.contains("ExecReload=/usr/bin/kill -HUP $MAINPID\n"),
            "{rendered}"
        );
    }

    #[test]
    fn present_exec_start_is_kept() {
        let text = "ExecStart=/usr/bin/true\n";
        let rendered =
            with_runnable_exec_start(text, |path| path == Path::new("/usr/bin/true"), "/bin/true");
        assert_eq!(rendered, text);
    }

    #[test]
    fn unit_contract_and_no_dbus_activation_file() {
        let unit = include_str!("../../packaging/stillwatch.service");
        for line in [
            "PartOf=graphical-session.target",
            "After=graphical-session.target",
            "WantedBy=graphical-session.target",
            "ExecStart=/usr/bin/stillwatchd",
            "ExecReload=/usr/bin/kill -HUP $MAINPID",
            "Restart=on-failure",
            "RestartSec=5s",
        ] {
            assert!(unit.contains(line), "missing {line}");
        }
        assert!(unit.contains("D-Bus activation is intentionally omitted"));
        assert!(unit.contains("JOURNAL_STREAM"));
        let root = workspace::root().unwrap();
        assert!(
            !root
                .join("packaging/io.github.jslay88.Stillwatch.service")
                .exists()
        );
    }

    #[test]
    fn desktop_files_match_the_launcher_and_screenshot_grant() {
        let gui = include_str!("../../packaging/io.github.jslay88.Stillwatch.desktop");
        assert!(gui.contains("Exec=stillwatch-gui settings\n"), "{gui}");
        let tray = include_str!("../../packaging/io.github.jslay88.Stillwatch.Tray.desktop");
        assert!(tray.contains("Exec=stillwatch-gui\n"), "{tray}");
        assert!(tray.contains("X-GNOME-Autostart-enabled=true"), "{tray}");
        let daemon = include_str!("../../packaging/io.github.jslay88.Stillwatch.Daemon.desktop");
        assert!(daemon.contains("Exec=/usr/bin/stillwatchd\n"), "{daemon}");
        assert!(
            daemon.contains("X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2"),
            "{daemon}"
        );
        assert!(daemon.contains("NoDisplay=true"), "{daemon}");
    }

    #[test]
    fn a_missing_user_manager_is_not_a_unit_error() {
        assert!(super::user_manager_unavailable(
            "Failed to initialize manager: No such device or address"
        ));
        assert!(super::user_manager_unavailable(
            "Failed to connect to bus: No medium found"
        ));
        assert!(!super::user_manager_unavailable(
            "stillwatch.service: Service has no ExecStart="
        ));
    }

    #[test]
    fn checkers_accept_the_packaging_files_when_installed() {
        if !on_path("systemd-analyze") || !on_path("desktop-file-validate") {
            eprintln!("skipping: packaging checkers are not installed");
            return;
        }
        check(&workspace::root().unwrap()).unwrap();
    }
}
