//! One connection's worth of overlays: bind the globals, restore whatever
//! should be covered, then serve requests and compositor events until the
//! connection ends.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use smithay_client_toolkit::compositor::CompositorState;
use smithay_client_toolkit::output::OutputState;
use smithay_client_toolkit::registry::RegistryState;
use smithay_client_toolkit::shell::wlr_layer::LayerShell;
use smithay_client_toolkit::shm::Shm;
use stillwatch_core::backend::{BackendError, EventSink};
use tokio::sync::{mpsc, oneshot, watch};
use wayland_client::globals::{BindError, GlobalError, GlobalList, registry_queue_init};
use wayland_client::{Connection, EventQueue};

use super::Shade;
use super::state::{OverlayState, lock};
use super::surface::Protocols;
use super::targets::{Desired, missing, selects};
use crate::wayland::{EventPump, dispatch_error, wayland_error};

/// How long the compositor gets to configure and show a new overlay.
pub const SHOW_TIMEOUT: Duration = Duration::from_secs(3);

/// The answer to a request.
pub type Reply = oneshot::Sender<Result<(), BackendError>>;

/// Work for the session, from the blanker.
pub enum Request {
    /// Cover `outputs` (empty = all) with `shade` and answer once it shows.
    Cover {
        /// Connector names.
        outputs: Vec<String>,
        /// What to show.
        shade: Shade,
        /// Where the answer goes.
        reply: Reply,
    },
    /// Bring the overlays in line with [`Desired`] after it shrank.
    Prune {
        /// Answered once the requests are sent.
        reply: Reply,
    },
}

/// Whether a session is up to take requests.
#[derive(Debug, Clone)]
pub enum Link {
    /// No session yet, or between reconnects.
    Connecting,
    /// A session is serving requests here.
    Up(mpsc::UnboundedSender<Request>),
    /// The last session failed for good, for example with no layer shell.
    Failed(BackendError),
}

/// Runs overlays on `conn` until the connection fails, publishing the
/// session on `link` once it's ready. Every overlay showing when it ends is
/// reported as gone.
///
/// # Errors
///
/// [`BackendError::Unsupported`] without `zwlr_layer_shell_v1`,
/// [`BackendError::Disconnected`] when the connection drops.
pub async fn run(
    conn: &Connection,
    sink: Arc<dyn EventSink>,
    desired: Arc<Mutex<Desired>>,
    link: &watch::Sender<Link>,
) -> Result<(), BackendError> {
    let init = conn.clone();
    let (mut state, queue) = tokio::task::spawn_blocking(move || connect(&init, sink, desired))
        .await
        .map_err(|error| BackendError::Io(error.to_string()))??;
    let mut pump = EventPump::with_queue(conn, queue)?;
    let qh = pump.handle();

    let (requests, mut inbox) = mpsc::unbounded_channel();
    let result = async {
        link.send_replace(Link::Up(requests));
        // An unblank may have landed while the session was starting.
        state.prune(&qh);
        serve(&mut pump, &mut state, &mut inbox).await
    }
    .await;
    state.lose_all();
    result
}

/// Reads the globals and the outputs' names. Blocks for two round trips, so
/// it runs off the async threads. Outputs that should be covered get their
/// overlays here, from `new_output`.
fn connect(
    conn: &Connection,
    sink: Arc<dyn EventSink>,
    desired: Arc<Mutex<Desired>>,
) -> Result<(OverlayState, EventQueue<OverlayState>), BackendError> {
    let (globals, mut queue) = registry_queue_init::<OverlayState>(conn).map_err(global_error)?;
    let qh = queue.handle();
    let mut state = OverlayState {
        registry: RegistryState::new(&globals),
        outputs: OutputState::new(&globals, &qh),
        protocols: bind(&globals, &qh)?,
        desired,
        sink,
        overlays: Vec::new(),
    };
    queue.roundtrip(&mut state).map_err(dispatch_error)?;
    Ok((state, queue))
}

fn bind(
    globals: &GlobalList,
    qh: &wayland_client::QueueHandle<OverlayState>,
) -> Result<Protocols, BackendError> {
    fn missing(name: &'static str) -> impl Fn(BindError) -> BackendError {
        move |error| BackendError::Unsupported(format!("{name}: {error}"))
    }
    Ok(Protocols {
        compositor: CompositorState::bind(globals, qh).map_err(missing("wl_compositor"))?,
        layer_shell: LayerShell::bind(globals, qh).map_err(missing(
            "the compositor has no wlr-layer-shell (zwlr_layer_shell_v1)",
        ))?,
        shm: Shm::bind(globals, qh).map_err(missing("wl_shm"))?,
        viewporter: globals.bind(qh, 1..=1, ()).ok(),
    })
}

enum Step {
    Request(Option<Request>),
    Turned(Result<(), BackendError>),
}

async fn serve(
    pump: &mut EventPump<OverlayState>,
    state: &mut OverlayState,
    inbox: &mut mpsc::UnboundedReceiver<Request>,
) -> Result<(), BackendError> {
    loop {
        let step = tokio::select! {
            biased;
            request = inbox.recv() => Step::Request(request),
            turned = pump.turn(state) => Step::Turned(turned),
        };
        match step {
            Step::Request(Some(Request::Cover {
                outputs,
                shade,
                reply,
            })) => {
                let answer = cover(pump, state, &outputs, shade).await?;
                let _ = reply.send(answer);
            }
            Step::Request(Some(Request::Prune { reply })) => {
                state.prune(&pump.handle());
                pump.flush()?;
                let _ = reply.send(Ok(()));
            }
            Step::Request(None) => return Ok(()),
            Step::Turned(turned) => turned?,
        }
    }
}

/// Covers the present outputs `outputs` selects and waits for them to show.
/// The outer error ends the session; the inner one is the request's answer.
async fn cover(
    pump: &mut EventPump<OverlayState>,
    state: &mut OverlayState,
    outputs: &[String],
    shade: Shade,
) -> Result<Result<(), BackendError>, BackendError> {
    let present = state.present();
    let names: Vec<String> = present.iter().map(|(name, _)| name.clone()).collect();
    let absent = missing(outputs, &names);
    let targets: Vec<_> = present
        .into_iter()
        .filter(|(name, _)| selects(outputs, name))
        .collect();
    if targets.is_empty() {
        let wanted = if outputs.is_empty() {
            "any output".to_owned()
        } else {
            absent.join(", ")
        };
        return Ok(Err(BackendError::NotFound(format!(
            "no overlay target: {wanted}"
        ))));
    }
    if !absent.is_empty() {
        tracing::warn!(?absent, "not connected; covering them when they appear");
    }
    lock(&state.desired).cover(outputs, shade);
    let qh = pump.handle();
    for (name, wl_output) in &targets {
        state.cover(&qh, wl_output, name, shade);
    }

    let names: Vec<String> = targets.into_iter().map(|(name, _)| name).collect();
    let showing = pump.run_until(state, |state| state.settled(&names, shade));
    match tokio::time::timeout(SHOW_TIMEOUT, showing).await {
        Ok(result) => result?,
        Err(_) => {
            return Ok(Err(BackendError::Protocol(
                "the compositor didn't show the overlay in time".into(),
            )));
        }
    }
    let uncovered: Vec<&str> = names
        .iter()
        .filter(|name| !state.covered(name, shade))
        .map(String::as_str)
        .collect();
    if uncovered.is_empty() {
        tracing::info!(outputs = ?names, ?shade, "overlay up");
        Ok(Ok(()))
    } else {
        Ok(Err(BackendError::Protocol(format!(
            "the overlay didn't stay up on {}",
            uncovered.join(", ")
        ))))
    }
}

fn global_error(error: GlobalError) -> BackendError {
    match error {
        GlobalError::Backend(backend) => wayland_error(backend),
        invalid @ GlobalError::InvalidId(_) => BackendError::Protocol(invalid.to_string()),
    }
}
