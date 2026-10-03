//! One round trip over a Wayland connection to read the globals we select on.

use stillwatch_core::backend::BackendError;
use wayland_client::Connection;
use wayland_client::Proxy as _;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::{Dispatch, QueueHandle};
use wayland_protocols::ext::idle_notify::v1::client::ext_idle_notifier_v1::ExtIdleNotifierV1;
use wayland_protocols_plasma::dpms::client::org_kde_kwin_dpms_manager::OrgKdeKwinDpmsManager;

use super::facts::{WaylandFacts, read_wlr_capture};

const LAYER_SHELL: &str = "zwlr_layer_shell_v1";

/// Reads globals from `conn`. Blocks for one round trip, so callers run it
/// off the async worker.
///
/// # Errors
///
/// [`BackendError::Disconnected`] when the compositor doesn't answer.
pub fn read(conn: &Connection) -> Result<WaylandFacts, BackendError> {
    let (globals, _queue) = registry_queue_init::<RegistryProbe>(conn)
        .map_err(|error| BackendError::Disconnected(format!("Wayland registry: {error}")))?;
    Ok(from_list(globals.contents()))
}

fn from_list(contents: &GlobalListContents) -> WaylandFacts {
    contents.with_list(|globals| {
        let idle_notifier_version = globals
            .iter()
            .find(|global| global.interface == ExtIdleNotifierV1::interface().name)
            .map(|global| global.version);
        WaylandFacts {
            idle_notifier_version,
            kwin_dpms: globals
                .iter()
                .any(|global| global.interface == OrgKdeKwinDpmsManager::interface().name),
            layer_shell: globals.iter().any(|global| global.interface == LAYER_SHELL),
            wlr_capture: read_wlr_capture(globals),
        }
    })
}

struct RegistryProbe;

impl Dispatch<WlRegistry, GlobalListContents> for RegistryProbe {
    fn event(
        _state: &mut Self,
        _proxy: &WlRegistry,
        _event: wl_registry::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlRegistry, ()> for RegistryProbe {
    fn event(
        _state: &mut Self,
        _proxy: &WlRegistry,
        _event: wl_registry::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

/// Resolves when `conn` drops. Used to notice a compositor restart.
///
/// # Errors
///
/// [`BackendError::Disconnected`] when the socket closes, which is the
/// signal the caller wanted, and [`BackendError::Io`] when the socket can't
/// be watched.
pub async fn until_drop(conn: &Connection) -> Result<(), BackendError> {
    let mut pump = crate::wayland::EventPump::new(conn)?;
    let _registry = conn.display().get_registry(&pump.handle(), ());
    let mut state = RegistryProbe;
    loop {
        pump.turn(&mut state).await?;
    }
}
