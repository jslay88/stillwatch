//! Connects the backends to the state machine.
//!
//! Events from idle, gamepads, media, session, and the blanker watches go in.
//! The machine's commands (capture, prompt, blank, history, timers) come out
//! and this module runs them. `main` only parses arguments and installs
//! signal handlers.
//!
//! Capture runs only when the machine asks for it. With no capture backend
//! the daemon stays up on input idle and does not blank. A prompter that
//! returns [`Unavailable`](stillwatch_core::backend::BackendError::Unavailable)
//! or [`Unsupported`](stillwatch_core::backend::BackendError::Unsupported)
//! becomes `PromptFailed`. [`StylePrompter`](crate::prompt::StylePrompter)
//! owns the dialog fallback.
//!
//! Panel care counters live in the state machine. This loop loads
//! `panel.json`, restores them, writes them back (debounced), and flushes
//! on shutdown.

mod engine;
mod exec;
mod handle;
mod inbox;
mod parts;
mod platform;
mod probe_loop;
mod reload;
mod shared;
mod spawn;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context as _;
use stillwatch_core::activity::ActivitySettings;
use tokio::sync::{mpsc, watch};

use self::engine::{Engine, Wiring};
use self::handle::Handle;
use self::parts::assemble;
use self::shared::Shared;
use crate::config_watch::Reloader;
use crate::service::Service;
use crate::signals::SignalSource;

/// Loads config, claims the bus name, and runs until a stop signal.
///
/// # Errors
///
/// A bad config file (anything but a missing one, which means defaults),
/// no state directory for the history ring, or another daemon already
/// owning the bus name.
pub async fn run(path: PathBuf, signals: &mut impl SignalSource) -> anyhow::Result<()> {
    let (reloader, loaded) = Reloader::load(path).context("can't load the config")?;
    let built = assemble(reloader.config()).await?;
    let (out, inbox) = mpsc::unbounded_channel();
    let (activity_tx, _) = watch::channel(ActivitySettings::from(reloader.config()));
    let shared = Arc::new(Shared::new(&built, out.clone()));
    let handle = Arc::new(Handle::new(Arc::clone(&shared)));
    let service = Service::start(handle).await?;
    let panel = crate::panel::PanelStore::new(
        crate::panel::default_path().context("can't resolve the panel care file")?,
    );
    let engine = Engine::new(
        built,
        reloader,
        loaded,
        Wiring {
            shared,
            inbox,
            out,
            activity_tx,
            reload_signal: service.signals().clone(),
            states: Some(service.signals().clone()),
            panel,
        },
    );
    engine.run(signals).await;
    Ok(())
}

#[cfg(test)]
mod tests;
