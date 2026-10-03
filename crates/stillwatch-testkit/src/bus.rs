use std::io::{BufRead, BufReader, ErrorKind};
use std::path::PathBuf;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

use zbus::Connection;
use zbus::connection::Builder;

use crate::Error;

/// Set to `1` to make a missing `dbus-daemon` an error instead of a skip.
pub const REQUIRE_ENV: &str = "STILLWATCH_REQUIRE_DBUS";

const DAEMON: &str = "dbus-daemon";

/// The stock session bus config minus `<standard_session_servicedirs/>`, so a
/// call to a missing name fails right away instead of D-Bus activating
/// whatever the machine has installed for it.
const CONFIG: &str = r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <keep_umask/>
  <listen>unix:tmpdir=/tmp</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
"#;

/// A private session `dbus-daemon` with no activatable services, killed when
/// dropped.
#[derive(Debug)]
pub struct PrivateBus {
    child: Child,
    // dbus-daemon gets SIGPIPE if its stdout closes while it's running.
    _stdout: BufReader<ChildStdout>,
    address: String,
}

impl PrivateBus {
    /// Starts a private session bus.
    ///
    /// Returns `Ok(None)`, after printing why to stderr, when `dbus-daemon`
    /// isn't installed and [`REQUIRE_ENV`] isn't `1`.
    ///
    /// # Errors
    ///
    /// Fails if `dbus-daemon` is missing while required, can't be spawned,
    /// or exits without printing an address.
    pub fn start() -> Result<Option<Self>, Error> {
        Self::start_program(DAEMON, required())
    }

    fn start_program(program: &str, required: bool) -> Result<Option<Self>, Error> {
        Self::spawn(Command::new(program), required)
    }

    /// Starts the bus from `command` (a `dbus-daemon` the caller has given an
    /// environment).
    pub(crate) fn spawn(mut command: Command, required: bool) -> Result<Option<Self>, Error> {
        let program = command.get_program().to_string_lossy().into_owned();
        let program = program.as_str();
        let config = ConfigFile::write()?;
        let spawned = command
            .arg(format!("--config-file={}", config.0.display()))
            .args(["--nofork", "--print-address"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .spawn();
        let mut child = match spawned {
            Ok(child) => child,
            Err(err) if err.kind() == ErrorKind::NotFound => {
                if required {
                    return Err(Error::Missing(program.to_owned()));
                }
                eprintln!("skipping: {program} isn't installed (set {REQUIRE_ENV}=1 to fail)");
                return Ok(None);
            }
            Err(err) => return Err(err.into()),
        };
        let Some(stdout) = child.stdout.take() else {
            stop(&mut child);
            return Err(Error::NoAddress);
        };
        let mut stdout = BufReader::new(stdout);
        let mut line = String::new();
        let read = stdout.read_line(&mut line);
        let address = line.trim().to_owned();
        if read.is_err() || address.is_empty() {
            stop(&mut child);
            return Err(read.err().map_or(Error::NoAddress, Error::Io));
        }
        Ok(Some(Self {
            child,
            _stdout: stdout,
            address,
        }))
    }

    /// The bus address, for `zbus::connection::Builder::address`.
    #[must_use]
    pub fn address(&self) -> &str {
        &self.address
    }

    /// Opens a new connection to the bus.
    ///
    /// # Errors
    ///
    /// Fails if the bus refuses the connection or has stopped.
    pub async fn connect(&self) -> Result<Connection, Error> {
        Ok(Builder::address(self.address())?.build().await?)
    }

    /// Kills the bus daemon now, dropping every connection to it.
    pub fn stop(&mut self) {
        stop(&mut self.child);
    }

    pub(crate) fn pid(&self) -> u32 {
        self.child.id()
    }
}

/// Whether [`REQUIRE_ENV`] is `1`.
pub(crate) fn required() -> bool {
    std::env::var(REQUIRE_ENV).is_ok_and(|value| value == "1")
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        stop(&mut self.child);
    }
}

/// The bus config on disk, deleted when dropped. `dbus-daemon` has read it
/// by the time it prints its address.
struct ConfigFile(PathBuf);

impl ConfigFile {
    fn write() -> std::io::Result<Self> {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let name = format!(
            "stillwatch-testkit-bus-{}-{}.conf",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(name);
        std::fs::write(&path, CONFIG)?;
        Ok(Self(path))
    }
}

impl Drop for ConfigFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn stop(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;

    const MISSING: &str = "stillwatch-testkit-no-such-dbus-daemon";

    #[test]
    fn a_missing_daemon_skips_unless_required() {
        assert!(PrivateBus::start_program(MISSING, false).unwrap().is_none());
        let err = PrivateBus::start_program(MISSING, true).unwrap_err();
        assert!(matches!(err, Error::Missing(ref name) if name == MISSING));
        assert!(err.to_string().contains(REQUIRE_ENV));
    }

    #[test]
    fn a_daemon_that_prints_nothing_is_an_error() {
        let err = PrivateBus::start_program("true", true).unwrap_err();
        assert!(matches!(err, Error::NoAddress), "{err}");
    }

    #[tokio::test]
    async fn starts_a_bus_that_accepts_connections() {
        let Some(mut bus) = PrivateBus::start().unwrap() else {
            return;
        };
        assert!(bus.address().starts_with("unix:"), "{}", bus.address());
        let conn = bus.connect().await.unwrap();
        assert!(conn.unique_name().is_some());
        bus.stop();
        assert!(bus.connect().await.is_err());
    }

    #[tokio::test]
    async fn missing_names_are_never_activated() {
        let Some(bus) = PrivateBus::start().unwrap() else {
            return;
        };
        let conn = bus.connect().await.unwrap();
        let name = "org.freedesktop.Notifications";
        let err = conn
            .call_method(Some(name), "/", Some(name), "GetCapabilities", &())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("ServiceUnknown"), "{err}");
    }
}
