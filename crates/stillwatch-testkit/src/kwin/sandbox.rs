//! The private directories and environment `KWin` and its clients run with.

use std::ffi::{OsStr, OsString};
use std::fs::{self, DirBuilder};
use std::io;
use std::os::unix::fs::DirBuilderExt as _;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

/// Passed through from the test's environment. Everything else is dropped so
/// nothing reaches the desktop session the tests happen to run in.
const FORWARD: &[&str] = &[
    "PATH",
    "LD_LIBRARY_PATH",
    "LANG",
    "LC_ALL",
    "XDG_DATA_DIRS",
    "XDG_CONFIG_DIRS",
    "QT_LOGGING_RULES",
];

/// Prefixes of renderer and `KWin` knobs, such as `KWIN_COMPOSE` or
/// `LIBGL_ALWAYS_SOFTWARE`, that are passed through too.
const FORWARD_PREFIXES: &[&str] = &["KWIN_", "LIBGL_", "MESA_", "GALLIUM_", "EGL_"];

/// Name of the Wayland socket inside the private runtime directory.
pub const SOCKET: &str = "wayland-stillwatch";

/// A temporary home, runtime directory, and XDG base directories.
#[derive(Debug)]
pub struct Sandbox {
    root: TempDir,
}

impl Sandbox {
    /// Creates the directories. The runtime directory is mode 0700, as
    /// Wayland requires.
    pub fn new() -> io::Result<Self> {
        let root = tempfile::Builder::new()
            .prefix("stillwatch-kwin-")
            .tempdir()?;
        let sandbox = Self { root };
        DirBuilder::new()
            .mode(0o700)
            .create(sandbox.runtime_dir())?;
        for dir in ["home", "config", "data", "cache", "state"] {
            fs::create_dir(sandbox.root.path().join(dir))?;
        }
        Ok(sandbox)
    }

    pub fn runtime_dir(&self) -> PathBuf {
        self.root.path().join("run")
    }

    pub fn data_home(&self) -> PathBuf {
        self.root.path().join("data")
    }

    pub fn socket_path(&self) -> PathBuf {
        self.runtime_dir().join(SOCKET)
    }

    pub fn log_path(&self) -> PathBuf {
        self.root.path().join("kwin.log")
    }

    /// What `KWin` itself runs with. No `WAYLAND_DISPLAY`: `KWin` is the server.
    pub fn server_env(&self, bus_address: Option<&str>) -> Vec<(OsString, OsString)> {
        let mut env = forwarded(std::env::vars_os());
        let root = self.root.path();
        let dirs = [
            ("HOME", "home"),
            ("XDG_RUNTIME_DIR", "run"),
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_DATA_HOME", "data"),
            ("XDG_CACHE_HOME", "cache"),
            ("XDG_STATE_HOME", "state"),
        ];
        env.extend(
            dirs.iter()
                .map(|(key, dir)| (key.into(), root.join(dir).into_os_string())),
        );
        if let Some(address) = bus_address {
            env.push(("DBUS_SESSION_BUS_ADDRESS".into(), address.into()));
        }
        env
    }

    /// What clients of `KWin` run with.
    pub fn client_env(&self, bus_address: Option<&str>) -> Vec<(OsString, OsString)> {
        let mut env = self.server_env(bus_address);
        env.extend(
            [
                ("WAYLAND_DISPLAY", SOCKET),
                ("XDG_SESSION_TYPE", "wayland"),
                ("XDG_CURRENT_DESKTOP", "KDE"),
                ("QT_QPA_PLATFORM", "wayland"),
            ]
            .map(|(key, value)| (key.into(), value.into())),
        );
        env
    }
}

fn forwarded(vars: impl Iterator<Item = (OsString, OsString)>) -> Vec<(OsString, OsString)> {
    vars.filter(|(key, _)| forward(key)).collect()
}

fn forward(key: &OsStr) -> bool {
    let Some(key) = key.to_str() else {
        return false;
    };
    FORWARD.contains(&key) || FORWARD_PREFIXES.iter().any(|p| key.starts_with(p))
}

/// The last `lines` lines of the file at `path`, or a note that it's unreadable.
pub fn tail(path: &Path, lines: usize) -> String {
    match fs::read_to_string(path) {
        Ok(text) => {
            let all: Vec<&str> = text.lines().collect();
            all[all.len().saturating_sub(lines)..].join("\n")
        }
        Err(err) => format!("(can't read {}: {err})", path.display()),
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt as _;

    use super::*;

    fn lookup<'a>(env: &'a [(OsString, OsString)], key: &str) -> Option<&'a OsStr> {
        env.iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_os_str())
    }

    #[test]
    fn only_renderer_and_path_vars_are_forwarded() {
        let vars = [
            ("PATH", "/usr/bin"),
            ("KWIN_COMPOSE", "Q"),
            ("LIBGL_ALWAYS_SOFTWARE", "1"),
            ("WAYLAND_DISPLAY", "wayland-0"),
            ("DISPLAY", ":0"),
            ("DBUS_SESSION_BUS_ADDRESS", "unix:path=/run/user/1000/bus"),
            ("XDG_RUNTIME_DIR", "/run/user/1000"),
        ]
        .map(|(k, v)| (OsString::from(k), OsString::from(v)));
        let kept: Vec<_> = forwarded(vars.into_iter())
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert_eq!(kept, ["PATH", "KWIN_COMPOSE", "LIBGL_ALWAYS_SOFTWARE"]);
    }

    #[test]
    fn everything_points_into_the_sandbox() {
        let sandbox = Sandbox::new().unwrap();
        let mode = fs::metadata(sandbox.runtime_dir())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700);

        let server = sandbox.server_env(None);
        assert_eq!(lookup(&server, "WAYLAND_DISPLAY"), None);
        assert_eq!(lookup(&server, "DBUS_SESSION_BUS_ADDRESS"), None);
        for key in ["HOME", "XDG_RUNTIME_DIR", "XDG_DATA_HOME", "XDG_CACHE_HOME"] {
            let dir = Path::new(lookup(&server, key).unwrap());
            assert!(dir.starts_with(sandbox.root.path()), "{key}");
            assert!(dir.is_dir(), "{key}");
        }

        let client = sandbox.client_env(Some("unix:path=/tmp/bus"));
        assert_eq!(lookup(&client, "WAYLAND_DISPLAY"), Some(OsStr::new(SOCKET)));
        assert_eq!(
            lookup(&client, "DBUS_SESSION_BUS_ADDRESS"),
            Some(OsStr::new("unix:path=/tmp/bus"))
        );
    }

    #[test]
    fn tail_keeps_the_last_lines() {
        let sandbox = Sandbox::new().unwrap();
        let log = sandbox.log_path();
        assert!(tail(&log, 2).contains("can't read"));
        fs::write(&log, "one\ntwo\nthree\n").unwrap();
        assert_eq!(tail(&log, 2), "two\nthree");
        assert_eq!(tail(&log, 10), "one\ntwo\nthree");
    }
}
