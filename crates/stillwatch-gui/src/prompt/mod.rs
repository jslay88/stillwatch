//! `stillwatch-gui prompt`: a short-lived dialog that answers over D-Bus.
//!
//! This process does not take the tray's bus name, so it can sit beside the
//! running GUI. The daemon kills it to dismiss the prompt. Buttons call
//! `PromptAnswer` and the process exits 0; the launcher then waits without
//! reporting that click again. Closing the window with no button is
//! `Dismissed`. A failed send exits non-zero so the launcher can still
//! report a dismissal or a failure.

mod app;
mod model;
mod text;
mod view;
mod watch;

pub use model::{Dialog, Input, Note, Step, update};

use std::sync::Arc;
use std::sync::atomic::AtomicI32;

use stillwatch_ipc::proxy::StillwatchProxy;
use zbus::proxy::CacheProperties;

use crate::args::{Cli, Command};
use crate::error::Error;

/// How this prompt process was started.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Launch {
    remaining: Option<u64>,
    custom: bool,
    address: Option<String>,
}

impl Launch {
    fn from_cli(cli: &Cli) -> Self {
        let (remaining, custom) = match &cli.command {
            Some(Command::Prompt { remaining, custom }) => (*remaining, *custom),
            _ => (None, false),
        };
        Self {
            remaining,
            custom,
            address: cli.bus_address.clone(),
        }
    }
}

/// Opens the prompt dialog and returns the process exit code.
///
/// `0` means an answer was sent, or the prompt had already resolved.
/// `1` means the window closed and the dismissal could not be sent.
/// `2` means some other answer could not be sent.
///
/// # Errors
///
/// Returns an iced failure. A missing daemon is not one: the window still
/// opens, and a click that can't be delivered becomes a non-zero exit.
pub fn run(cli: &Cli) -> anyhow::Result<i32> {
    let code = Arc::new(AtomicI32::new(0));
    let launch = Launch::from_cli(cli);
    let boot_code = Arc::clone(&code);
    app::run(launch, boot_code)?;
    Ok(code.load(std::sync::atomic::Ordering::SeqCst))
}

/// Sends `PromptAnswer(kind, minutes)` on an existing proxy.
///
/// # Errors
///
/// Returns the D-Bus error from the call.
pub async fn answer_prompt(
    proxy: &StillwatchProxy<'_>,
    kind: stillwatch_ipc::prompt::PromptAnswerKind,
    minutes: u32,
) -> Result<(), Error> {
    proxy
        .prompt_answer(kind.as_str(), minutes)
        .await
        .map_err(Error::from)
}

pub(crate) async fn answer_at(
    address: Option<&str>,
    kind: stillwatch_ipc::prompt::PromptAnswerKind,
    minutes: u32,
) -> Result<(), Error> {
    let connection = crate::bus::connection(address).await?;
    let proxy = StillwatchProxy::builder(&connection)
        .cache_properties(CacheProperties::No)
        .build()
        .await?;
    answer_prompt(&proxy, kind, minutes).await
}
