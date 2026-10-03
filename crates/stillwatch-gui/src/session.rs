//! Claims the GUI bus name and watches the daemon on that connection.

use tokio::sync::mpsc;
use zbus::Connection;

use crate::daemon;
use crate::instance::{self, Claim};
use crate::launch::LaunchMode;
use crate::shell::{DaemonCall, DaemonEvent};

/// What [`start`] decided, plus the daemon event stream when we are primary.
pub struct Started {
    /// Whether this process should run, exit, or open a window with no bus.
    pub outcome: Outcome,
    /// Snapshots and call failures from [`daemon::watch`]. Empty unless
    /// [`Outcome::Primary`].
    pub daemon: mpsc::Receiver<DaemonEvent>,
}

/// Result of trying to become the running GUI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// This process owns the GUI name. The daemon watcher is running.
    Primary,
    /// Another GUI is running and has been asked to show the window.
    HandedOff,
    /// The bus couldn't be used. The window can still open.
    NoBus(String),
}

/// Connects, claims the single-instance name, and on success follows the daemon.
pub async fn start(
    address: Option<&str>,
    mode: LaunchMode,
    activations: mpsc::Sender<LaunchMode>,
    calls: mpsc::Receiver<DaemonCall>,
) -> Started {
    let (daemon_tx, daemon_rx) = mpsc::channel(32);
    let Ok(connection) = crate::bus::connection(address).await else {
        return no_bus(daemon_rx, "can't connect to the D-Bus session bus");
    };
    match instance::claim(&connection, mode, activations).await {
        Ok(Claim::Primary) => {
            spawn_watch(connection, calls, daemon_tx);
            Started {
                outcome: Outcome::Primary,
                daemon: daemon_rx,
            }
        }
        Ok(Claim::HandedOff) => Started {
            outcome: Outcome::HandedOff,
            daemon: daemon_rx,
        },
        Err(err) => no_bus(daemon_rx, &err.to_string()),
    }
}

fn no_bus(daemon: mpsc::Receiver<DaemonEvent>, message: &str) -> Started {
    Started {
        outcome: Outcome::NoBus(message.to_owned()),
        daemon,
    }
}

fn spawn_watch(
    connection: Connection,
    calls: mpsc::Receiver<DaemonCall>,
    events: mpsc::Sender<DaemonEvent>,
) {
    tokio::spawn(async move {
        daemon::watch(connection, calls, events).await;
    });
}
