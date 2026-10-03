//! Whether a `.desktop` file authorizes this executable for `ScreenShot2`.
//!
//! `KWin` compares the caller's `/proc/<pid>/exe` (symlinks followed) to the
//! first `Exec=` word of an application whose
//! `X-KDE-DBUS-Restricted-Interfaces` lists `org.kde.KWin.ScreenShot2`.

use std::fs;
use std::path::{Path, PathBuf};

const SCREENSHOT2: &str = "org.kde.KWin.ScreenShot2";
const INTERFACES: &str = "X-KDE-DBUS-Restricted-Interfaces";

/// `true` when some `applications/*.desktop` under `dirs` grants `exe`.
#[must_use]
pub fn screenshot_authorized(dirs: &[PathBuf], exe: &Path) -> bool {
    let Ok(exe) = fs::canonicalize(exe) else {
        return false;
    };
    dirs.iter().any(|dir| dir_grants(dir, &exe))
}

/// Standard application directories: `$XDG_DATA_HOME` and `$XDG_DATA_DIRS`.
#[must_use]
pub fn application_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = data_home() {
        dirs.push(home.join("applications"));
    }
    let data_dirs =
        std::env::var_os("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    for dir in std::env::split_paths(&data_dirs) {
        dirs.push(dir.join("applications"));
    }
    dirs
}

fn data_home() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
}

fn dir_grants(dir: &Path, exe: &Path) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("desktop") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        if grant_matches(&text, exe) {
            return true;
        }
    }
    false
}

fn grant_matches(text: &str, exe: &Path) -> bool {
    let mut exec = None;
    let mut interfaces = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('[') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "Exec" => exec = Some(first_exec_word(value.trim())),
            INTERFACES => interfaces = Some(value.trim().to_owned()),
            _ => {}
        }
    }
    let Some(exec) = exec else {
        return false;
    };
    let Some(interfaces) = interfaces else {
        return false;
    };
    interface_listed(&interfaces) && paths_match(&exec, exe)
}

fn first_exec_word(value: &str) -> String {
    if let Some(rest) = value.strip_prefix('"') {
        return rest.split('"').next().unwrap_or("").to_owned();
    }
    value.split_whitespace().next().unwrap_or("").to_owned()
}

fn interface_listed(value: &str) -> bool {
    value
        .split([',', ';'])
        .any(|part| part.trim() == SCREENSHOT2)
}

fn paths_match(exec: &str, exe: &Path) -> bool {
    fs::canonicalize(exec).is_ok_and(|path| path == exe)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exe_file() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("stillwatchd");
        fs::write(&exe, "").unwrap();
        (dir, fs::canonicalize(exe).unwrap())
    }

    fn write_desktop(dir: &Path, body: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("grant.desktop"), body).unwrap();
    }

    #[test]
    fn a_matching_grant_authorizes_the_exe() {
        let (dir, exe) = exe_file();
        let apps = dir.path().join("applications");
        write_desktop(
            &apps,
            &format!(
                "[Desktop Entry]\nExec={}\nX-KDE-DBUS-Restricted-Interfaces={SCREENSHOT2}\n",
                exe.display()
            ),
        );
        assert!(screenshot_authorized(&[apps], &exe));
    }

    #[test]
    fn quoted_exec_and_a_semicolon_list_match() {
        let (dir, exe) = exe_file();
        let apps = dir.path().join("applications");
        write_desktop(
            &apps,
            &format!(
                "Exec=\"{}\" --flag\nX-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.Other;{SCREENSHOT2};\n",
                exe.display()
            ),
        );
        assert!(screenshot_authorized(std::slice::from_ref(&apps), &exe));
        write_desktop(
            &apps,
            &format!(
                "Exec={}\nX-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.Other\n",
                exe.display()
            ),
        );
        assert!(!screenshot_authorized(&[apps], &exe));
    }

    #[test]
    fn a_different_executable_is_not_authorized() {
        let (dir, exe) = exe_file();
        let other = dir.path().join("other");
        fs::write(&other, "").unwrap();
        let apps = dir.path().join("applications");
        write_desktop(
            &apps,
            &format!(
                "Exec={}\nX-KDE-DBUS-Restricted-Interfaces={SCREENSHOT2}\n",
                other.display()
            ),
        );
        assert!(!screenshot_authorized(&[apps], &exe));
        assert!(!screenshot_authorized(&[dir.path().join("missing")], &exe));
    }
}
