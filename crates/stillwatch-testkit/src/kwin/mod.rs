//! A headless `kwin_wayland --virtual` per test.
//!
//! [`Kwin::start`] creates a sandbox (temporary home, `XDG_RUNTIME_DIR`, and
//! XDG base directories), starts a [`PrivateBus`] in it, launches `KWin` on that
//! bus with a fixed virtual output size, and waits until it answers on
//! Wayland and owns `org.kde.KWin` on the bus. Both processes run with a
//! cleared environment, so neither can reach the desktop session the tests
//! happen to run in, and the bus activates nothing. Dropping the
//! [`Kwin`] kills `KWin`, the bus, and everything they started.
//!
//! When `kwin_wayland` isn't installed, [`Kwin::start`] returns `Ok(None)`
//! and the test should return early, unless `STILLWATCH_REQUIRE_KWIN=1` is
//! set (as in CI), which turns that into an error.

mod desktop;
mod reap;
mod sandbox;
mod wayland;

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use tokio::time::{Instant, sleep};
use wayland_client::Connection;
use wayland_client::globals::Global;
use zbus::fdo::DBusProxy;
use zbus::names::BusName;

pub use self::desktop::{Authorization, SCREENSHOT2};
use self::sandbox::{Sandbox, tail};
use crate::{Error, PrivateBus, bus};

/// Set to `1` to make a missing `kwin_wayland` an error instead of a skip.
pub const REQUIRE_ENV: &str = "STILLWATCH_REQUIRE_KWIN";

/// The bus name `KWin` owns once its D-Bus interfaces are up.
pub const KWIN_BUS_NAME: &str = "org.kde.KWin";

const PROGRAM: &str = "kwin_wayland";

const POLL: Duration = Duration::from_millis(50);

const LOG_LINES: usize = 60;

/// How the virtual session is set up.
#[derive(Clone, Debug)]
pub struct KwinOptions {
    /// Width of each virtual output in pixels.
    pub width: u32,
    /// Height of each virtual output in pixels.
    pub height: u32,
    /// Number of virtual outputs, named `Virtual-0`, `Virtual-1`, and so on.
    pub outputs: u32,
    /// How long `KWin` gets to come up. Software rendering on a cold CI runner
    /// is slow.
    pub ready_timeout: Duration,
    /// Executables allowed to call restricted interfaces such as
    /// [`SCREENSHOT2`], installed before `KWin` starts.
    pub authorize: Vec<Authorization>,
}

impl Default for KwinOptions {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            outputs: 1,
            ready_timeout: Duration::from_secs(30),
            authorize: Vec::new(),
        }
    }
}

/// A running headless `KWin` on a private session bus, killed when dropped.
#[derive(Debug)]
pub struct Kwin {
    child: Child,
    bus: PrivateBus,
    sandbox: Sandbox,
}

impl Kwin {
    /// Starts `KWin` and waits until it's ready.
    ///
    /// Returns `Ok(None)`, after printing why to stderr, when `kwin_wayland`
    /// or `dbus-daemon` isn't installed and neither [`REQUIRE_ENV`] nor
    /// [`crate::REQUIRE_ENV`] is `1`.
    ///
    /// # Errors
    ///
    /// Fails if a required program is missing, `KWin` exits during startup, or
    /// it isn't ready within [`KwinOptions::ready_timeout`]. The error holds
    /// the end of `KWin`'s log.
    pub async fn start(options: KwinOptions) -> Result<Option<Self>, Error> {
        let args = arguments(&options);
        Self::launch(PROGRAM, &args, &options, required()).await
    }

    async fn launch(
        program: &str,
        args: &[String],
        options: &KwinOptions,
        required: bool,
    ) -> Result<Option<Self>, Error> {
        let sandbox = Sandbox::new()?;
        for (index, grant) in options.authorize.iter().enumerate() {
            desktop::install(&sandbox.data_home(), index, grant)?;
        }
        let mut daemon = Command::new("dbus-daemon");
        daemon.env_clear().envs(sandbox.client_env(None));
        let Some(bus) = PrivateBus::spawn(daemon, required || bus::required())? else {
            return Ok(None);
        };
        let log = File::create(sandbox.log_path())?;
        let spawned = Command::new(program)
            .args(args)
            .env_clear()
            .envs(sandbox.server_env(Some(bus.address())))
            .current_dir(sandbox.runtime_dir())
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log)
            .spawn();
        let child = match spawned {
            Ok(child) => child,
            Err(err) if err.kind() == ErrorKind::NotFound => {
                if required {
                    return Err(Error::KwinMissing(program.to_owned()));
                }
                eprintln!("skipping: {program} isn't installed (set {REQUIRE_ENV}=1 to fail)");
                return Ok(None);
            }
            Err(err) => return Err(err.into()),
        };
        let mut kwin = Self {
            child,
            bus,
            sandbox,
        };
        kwin.wait_ready(Instant::now() + options.ready_timeout)
            .await?;
        Ok(Some(kwin))
    }

    async fn wait_ready(&mut self, deadline: Instant) -> Result<(), Error> {
        let socket = self.sandbox.socket_path();
        loop {
            if let Some(status) = self.child.try_wait()? {
                return Err(Error::KwinExited {
                    status,
                    log: self.log(),
                });
            }
            if socket.exists() && wayland::globals(&socket, deadline).await.is_ok() {
                break;
            }
            if Instant::now() >= deadline {
                return Err(self.timeout("the Wayland socket"));
            }
            sleep(POLL).await;
        }
        self.wait_for_name_until(KWIN_BUS_NAME, deadline).await
    }

    /// The `WAYLAND_DISPLAY` value for clients, relative to
    /// [`runtime_dir`](Self::runtime_dir).
    #[must_use]
    pub fn wayland_display(&self) -> &'static str {
        sandbox::SOCKET
    }

    /// The absolute path of the Wayland socket.
    #[must_use]
    pub fn socket_path(&self) -> PathBuf {
        self.sandbox.socket_path()
    }

    /// The private `XDG_RUNTIME_DIR` holding the socket.
    #[must_use]
    pub fn runtime_dir(&self) -> PathBuf {
        self.sandbox.runtime_dir()
    }

    /// The private `XDG_DATA_HOME` `KWin` reads applications from.
    #[must_use]
    pub fn data_home(&self) -> PathBuf {
        self.sandbox.data_home()
    }

    /// The private session bus `KWin` is on.
    #[must_use]
    pub fn bus(&self) -> &PrivateBus {
        &self.bus
    }

    /// The `DBUS_SESSION_BUS_ADDRESS` of [`bus`](Self::bus).
    #[must_use]
    pub fn dbus_address(&self) -> &str {
        self.bus.address()
    }

    /// The complete environment for a client of this `KWin`: the sandbox's
    /// home and XDG directories, `WAYLAND_DISPLAY`, the bus address, and the
    /// few variables passed through from the test (`PATH`, renderer knobs).
    /// Use it with `env_clear`, as [`command`](Self::command) does.
    #[must_use]
    pub fn env(&self) -> Vec<(OsString, OsString)> {
        self.sandbox.client_env(Some(self.bus.address()))
    }

    /// A command for `program` that runs as a client of this `KWin`, with
    /// nothing from the test's own session in its environment.
    #[must_use]
    pub fn command(&self, program: impl AsRef<OsStr>) -> Command {
        let mut command = Command::new(program);
        command.env_clear().envs(self.env());
        command
    }

    /// Opens a Wayland connection to `KWin`.
    ///
    /// # Errors
    ///
    /// Fails if the socket refuses the connection.
    pub fn connect(&self) -> Result<Connection, Error> {
        wayland::connect(&self.socket_path())
    }

    /// A connection factory that outlives `self`, for backends that take a
    /// connector closure and reconnect on their own.
    pub fn connector(&self) -> impl Fn() -> Result<Connection, Error> + Send + Sync + 'static {
        let socket = self.socket_path();
        move || wayland::connect(&socket)
    }

    /// The globals `KWin` advertises right now.
    ///
    /// # Errors
    ///
    /// Fails if the connection fails or `KWin` doesn't answer within
    /// `timeout`.
    pub async fn globals(&self, timeout: Duration) -> Result<Vec<Global>, Error> {
        wayland::globals(&self.socket_path(), Instant::now() + timeout).await
    }

    /// Waits until `name` has an owner on the bus, such as
    /// [`SCREENSHOT2`], whose plugin may load after `KWin` reports ready.
    ///
    /// # Errors
    ///
    /// Fails if the bus fails or `name` has no owner within `timeout`.
    pub async fn wait_for_name(&self, name: &str, timeout: Duration) -> Result<(), Error> {
        self.wait_for_name_until(name, Instant::now() + timeout)
            .await
    }

    async fn wait_for_name_until(&self, name: &str, deadline: Instant) -> Result<(), Error> {
        let conn = self.bus.connect().await?;
        let proxy = DBusProxy::new(&conn).await?;
        let name = BusName::try_from(name).map_err(zbus::Error::from)?;
        loop {
            if proxy
                .name_has_owner(name.clone())
                .await
                .map_err(zbus::Error::from)?
            {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(self.timeout(&format!("{name} on the bus")));
            }
            sleep(POLL).await;
        }
    }

    /// The end of `KWin`'s stdout and stderr.
    #[must_use]
    pub fn log(&self) -> String {
        tail(&self.sandbox.log_path(), LOG_LINES)
    }

    fn timeout(&self, what: &str) -> Error {
        Error::Timeout {
            what: what.to_owned(),
            log: self.log(),
        }
    }
}

impl Drop for Kwin {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("kwin_wayland log:\n{}", self.log());
        }
        reap::kill_tree(&[self.child.id(), self.bus.pid()]);
        let _ = self.child.wait();
    }
}

/// Whether [`REQUIRE_ENV`] is `1`.
fn required() -> bool {
    std::env::var(REQUIRE_ENV).is_ok_and(|value| value == "1")
}

fn arguments(options: &KwinOptions) -> Vec<String> {
    let mut args: Vec<String> = [
        "--virtual",
        "--no-lockscreen",
        "--no-global-shortcuts",
        "--no-kactivities",
        "--socket",
        sandbox::SOCKET,
    ]
    .map(str::to_owned)
    .into();
    for (flag, value) in [
        ("--width", options.width),
        ("--height", options.height),
        ("--output-count", options.outputs),
    ] {
        args.extend([flag.to_owned(), value.to_string()]);
    }
    args
}

#[cfg(test)]
mod tests;
