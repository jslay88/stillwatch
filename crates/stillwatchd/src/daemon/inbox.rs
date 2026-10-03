//! Messages the event loop waits on.

use std::time::Duration;

use stillwatch_core::event::ControlCommand;
use stillwatch_core::event::Event;
use stillwatch_core::prompt::PromptOutcome;
use stillwatch_ipc::probe::ProbeSample;
use tokio::sync::{mpsc, oneshot};

use crate::config_watch::ReloadTrigger;
use crate::service::{DaemonStatus, ReloadReport};

/// One wake-up for the daemon loop.
pub(super) enum Incoming {
    /// A backend event, or a command result, for the state machine.
    Event(Event),
    /// A capture finished. Ignored when the generation is stale.
    Capture(u64, Event),
    /// A prompt finished. Ignored when the generation is stale.
    Prompt(u64, Event),
    /// Watcher, SIGHUP, or `Reload()`. `reply` is set for the D-Bus call.
    Reload(ReloadTrigger, Option<oneshot::Sender<ReloadReport>>),
    /// `Event::Control`, answered once the machine has taken it.
    Control(ControlCommand, oneshot::Sender<()>),
    /// `Event::PromptAnswered`, answered once the machine has taken it.
    Answer(PromptOutcome, oneshot::Sender<()>),
    /// A status snapshot as of now.
    Status(oneshot::Sender<DaemonStatus>),
    /// Start (or replace) the calibration probe.
    Probe(Duration, mpsc::Sender<ProbeSample>),
    /// The test clock moved. Production never sends this.
    #[cfg_attr(not(test), allow(dead_code))]
    Tick,
}
