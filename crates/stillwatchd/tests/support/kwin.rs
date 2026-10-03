//! A throwaway `kwin_wayland --virtual` for one test.
//!
//! Kept minimal on purpose; the shared headless `KWin` harness (JUS-16)
//! replaces it. `KWin` and every helper run with a cleared environment and a
//! private `XDG_RUNTIME_DIR`, under their own `dbus-run-session`, so nothing
//! here can reach the desktop session's compositor or bus.

use std::fs::File;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use stillwatch_core::backend::BackendError;
use tempfile::TempDir;
use wayland_client::Connection;

/// `KWin`'s stdout and stderr, in the runtime dir.
const LOG: &str = "kwin.log";
/// Where `KWin`'s private bus address is written, in the runtime dir.
const BUS: &str = "bus";
/// How long to wait for `KWin` to start or a script to answer.
const WAIT: Duration = Duration::from_secs(20);

/// Numbers sockets and scripts, so parallel tests don't collide.
static NEXT: AtomicU32 = AtomicU32::new(0);

/// Whether a missing `KWin` should fail instead of skip.
fn required() -> bool {
    std::env::var_os("STILLWATCH_REQUIRE_KWIN").is_some_and(|value| value == "1")
}

fn on_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(program).is_file()))
}

/// A running private `KWin`. Dropping it stops `KWin` and its bus.
pub struct Kwin {
    child: Option<Child>,
    runtime: TempDir,
    socket: String,
}

impl Kwin {
    /// Starts `KWin` with `outputs` virtual 1920x1080 outputs, or returns
    /// `None` (after saying why) when it isn't installed and
    /// `STILLWATCH_REQUIRE_KWIN=1` isn't set.
    pub fn start(outputs: u32) -> Option<Self> {
        let tools = ["kwin_wayland", "dbus-run-session"];
        if let Some(absent) = tools.iter().find(|tool| !on_path(tool)) {
            assert!(
                !required(),
                "STILLWATCH_REQUIRE_KWIN=1 but {absent} isn't installed"
            );
            eprintln!("skipping: {absent} isn't installed");
            return None;
        }
        let socket = format!(
            "stillwatch-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let runtime = tempfile::Builder::new()
            .prefix("sw-kwin-")
            .tempdir()
            .unwrap();
        let log = File::create(runtime.path().join(LOG)).unwrap();
        let script = format!(
            "echo \"$DBUS_SESSION_BUS_ADDRESS\" > {BUS}; \
             exec kwin_wayland --virtual --width 1920 --height 1080 \
             --output-count {outputs} --socket {socket}"
        );
        let child = isolated(Command::new("dbus-run-session"), runtime.path())
            .args(["--", "sh", "-c", &script])
            .current_dir(runtime.path())
            .env("QT_LOGGING_RULES", "kwin_scripting.debug=true")
            .env("QT_FORCE_STDERR_LOGGING", "1")
            .stdin(Stdio::null())
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .process_group(0)
            .spawn()
            .unwrap();
        let mut kwin = Self {
            child: Some(child),
            runtime,
            socket,
        };
        kwin.wait_for_socket();
        Some(kwin)
    }

    fn socket_path(&self) -> PathBuf {
        self.runtime.path().join(&self.socket)
    }

    fn wait_for_socket(&mut self) {
        let deadline = Instant::now() + WAIT;
        while UnixStream::connect(self.socket_path()).is_err() {
            if let Some(status) = self.child.as_mut().unwrap().try_wait().unwrap() {
                panic!("kwin_wayland exited before listening: {status}");
            }
            assert!(Instant::now() < deadline, "kwin_wayland didn't start");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// A connector for this `KWin`'s socket only.
    pub fn connector(
        &self,
    ) -> impl Fn() -> Result<Connection, BackendError> + Send + Sync + 'static {
        let path = self.socket_path();
        move || {
            let stream = UnixStream::connect(&path)
                .map_err(|error| BackendError::Disconnected(error.to_string()))?;
            Connection::from_socket(stream)
                .map_err(|error| BackendError::Unavailable(error.to_string()))
        }
    }

    /// Runs `kscreen-doctor` against this `KWin`, if it's installed.
    pub fn kscreen_doctor(&self, args: &[&str]) -> Option<()> {
        if !on_path("kscreen-doctor") {
            assert!(
                !required(),
                "STILLWATCH_REQUIRE_KWIN=1 but kscreen-doctor isn't installed"
            );
            eprintln!("skipping: kscreen-doctor isn't installed");
            return None;
        }
        let output = isolated(Command::new("kscreen-doctor"), self.runtime.path())
            .env("WAYLAND_DISPLAY", &self.socket)
            .env("QT_QPA_PLATFORM", "wayland")
            .args(args)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "kscreen-doctor {args:?}: {output:?}"
        );
        Some(())
    }

    /// Runs the `KWin` script `source` over this `KWin`'s private bus and
    /// returns what it `print`s, waiting until `lines` lines arrive. `None`
    /// (after saying why) without `dbus-send`.
    pub fn script(&self, source: &str, lines: usize) -> Option<Vec<String>> {
        if !on_path("dbus-send") {
            assert!(
                !required(),
                "STILLWATCH_REQUIRE_KWIN=1 but dbus-send isn't installed"
            );
            eprintln!("skipping: dbus-send isn't installed");
            return None;
        }
        let name = format!("stillwatch{}", NEXT.fetch_add(1, Ordering::Relaxed));
        let marker = format!("{name}: ");
        let path = self.runtime.path().join(format!("{name}.js"));
        let body = source.replace("print(", &format!("print({marker:?} + "));
        std::fs::write(&path, body).unwrap();

        let loaded = self.dbus(&[
            "/Scripting",
            "org.kde.kwin.Scripting.loadScript",
            &format!("string:{}", path.display()),
            &format!("string:{name}"),
        ]);
        let id = loaded.split_whitespace().last().unwrap().to_owned();
        self.dbus(&[&format!("/Scripting/Script{id}"), "org.kde.kwin.Script.run"]);

        let deadline = Instant::now() + WAIT;
        loop {
            let log = std::fs::read_to_string(self.runtime.path().join(LOG)).unwrap();
            let printed: Vec<String> = log
                .lines()
                .filter_map(|line| Some(line.split_once(&marker)?.1.to_owned()))
                .collect();
            if printed.len() >= lines {
                self.dbus(&[
                    "/Scripting",
                    "org.kde.kwin.Scripting.unloadScript",
                    &format!("string:{name}"),
                ]);
                return Some(printed);
            }
            assert!(Instant::now() < deadline, "script printed {printed:?}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn dbus(&self, args: &[&str]) -> String {
        let bus = std::fs::read_to_string(self.runtime.path().join(BUS)).unwrap();
        let output = isolated(Command::new("dbus-send"), self.runtime.path())
            .env("DBUS_SESSION_BUS_ADDRESS", bus.trim())
            .args(["--session", "--dest=org.kde.KWin", "--print-reply=literal"])
            .args(args)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(output.status.success(), "dbus-send {args:?}: {output:?}");
        String::from_utf8(output.stdout).unwrap()
    }

    /// Stops `KWin` now, as a crash or restart would.
    pub fn kill(&mut self) {
        if let Some(mut child) = self.child.take() {
            let group = format!("-{}", child.id());
            for signal in ["-TERM", "-KILL"] {
                let _ = Command::new("kill").args([signal, "--", &group]).status();
                let deadline = Instant::now() + Duration::from_secs(5);
                while child.try_wait().unwrap().is_none() && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
            let _ = child.wait();
        }
    }
}

impl Drop for Kwin {
    fn drop(&mut self) {
        self.kill();
    }
}

/// `command` with only what a private session needs in its environment.
fn isolated(mut command: Command, runtime: &Path) -> Command {
    command.env_clear();
    if let Some(path) = std::env::var_os("PATH") {
        command.env("PATH", path);
    }
    command
        .env("HOME", runtime)
        .env("XDG_RUNTIME_DIR", runtime)
        .env("XDG_CONFIG_HOME", runtime.join("config"));
    command
}
