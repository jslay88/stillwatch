use std::io::{BufRead, BufReader, ErrorKind};
use std::process::{Child, ChildStdout, Command, Stdio};

use zbus::Connection;
use zbus::connection::Builder;

use crate::Error;

/// Set to `1` to make a missing `dbus-daemon` an error instead of a skip.
pub const REQUIRE_ENV: &str = "STILLWATCH_REQUIRE_DBUS";

const DAEMON: &str = "dbus-daemon";

/// A private `dbus-daemon --session`, killed when dropped.
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
        let required = std::env::var(REQUIRE_ENV).is_ok_and(|value| value == "1");
        Self::start_program(DAEMON, required)
    }

    fn start_program(program: &str, required: bool) -> Result<Option<Self>, Error> {
        let spawned = Command::new(program)
            .args(["--session", "--nofork", "--print-address"])
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
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        stop(&mut self.child);
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
}
