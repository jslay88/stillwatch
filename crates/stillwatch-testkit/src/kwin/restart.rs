//! Kill the headless compositor and start it again in the same sandbox.

use std::fs::OpenOptions;
use std::process::{Child, Command, Stdio};

use tokio::time::Instant;

use super::reap;
use super::{Error, Kwin};

impl Kwin {
    /// SIGKILLs this `KWin` and starts another on the same socket and bus.
    ///
    /// The private bus stays up. Clients reconnect to the same
    /// `WAYLAND_DISPLAY`.
    ///
    /// # Errors
    ///
    /// Fails if the new process cannot be spawned or is not ready within the
    /// original timeout.
    pub async fn restart(&mut self) -> Result<(), Error> {
        reap::kill_tree(&[self.child.id()]);
        let _ = self.child.wait();
        let _ = std::fs::remove_file(self.sandbox.socket_path());
        self.child = spawn_child(&self.program, &self.args, &self.sandbox, self.bus.address())?;
        self.wait_ready(Instant::now() + self.ready_timeout).await
    }
}

pub(super) fn spawn_child(
    program: &str,
    args: &[String],
    sandbox: &super::Sandbox,
    bus_address: &str,
) -> std::io::Result<Child> {
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(sandbox.log_path())?;
    Command::new(program)
        .args(args)
        .env_clear()
        .envs(sandbox.server_env(Some(bus_address)))
        .current_dir(sandbox.runtime_dir())
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .spawn()
}
