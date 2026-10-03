//! One connection's worth of DPMS watching: bind `org_kde_kwin_dpms_manager`,
//! get a DPMS object for every output (including ones plugged in later), and
//! report each output's power state until the connection ends.

use std::sync::Arc;

use stillwatch_core::backend::{BackendError, EventSink};
use stillwatch_core::event::{Event, PowerKind};
use wayland_client::protocol::wl_callback::WlCallback;
use wayland_client::protocol::wl_output::WlOutput;
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::{
    Connection, Dispatch, Proxy as _, QueueHandle, delegate_dispatch, delegate_noop,
};
use wayland_protocols_plasma::dpms::client::org_kde_kwin_dpms::{self, OrgKdeKwinDpms};
use wayland_protocols_plasma::dpms::client::org_kde_kwin_dpms_manager::OrgKdeKwinDpmsManager;

use super::power::{self, PowerTracker};
use crate::wayland::outputs::{OutputChange, OutputGlobal, OutputRegistry};
use crate::wayland::{EventPump, Synced};

/// The manager version Stillwatch knows.
const MANAGER_VERSION: u32 = 1;

struct OutputDpms {
    global: OutputGlobal,
    proxy: OrgKdeKwinDpms,
    pending: Option<u32>,
    mode: Option<u32>,
}

/// Dispatch state for one connection.
struct PowerState {
    sink: Arc<dyn EventSink>,
    outputs: OutputRegistry,
    manager: Option<(u32, OrgKdeKwinDpmsManager)>,
    manager_removed: bool,
    dpms: Vec<OutputDpms>,
    tracker: PowerTracker,
}

/// Reports DPMS changes on `conn` until the connection fails.
///
/// Never returns `Ok`: a healthy connection keeps reporting forever.
pub async fn watch_on(conn: &Connection, sink: Arc<dyn EventSink>) -> Result<(), BackendError> {
    let mut pump = EventPump::new(conn)?;
    let qh = pump.handle();
    let mut state = PowerState {
        sink,
        outputs: OutputRegistry::default(),
        manager: None,
        manager_removed: false,
        dpms: Vec::new(),
        tracker: PowerTracker::default(),
    };
    let _registry = conn.display().get_registry(&qh, ());
    pump.roundtrip(conn, &mut state).await?;
    if state.manager.is_none() {
        return Err(BackendError::Unavailable(format!(
            "compositor doesn't advertise {}",
            OrgKdeKwinDpmsManager::interface().name
        )));
    }
    tracing::info!(outputs = state.dpms.len(), "watching DPMS");

    loop {
        state.report();
        if state.manager_removed {
            return Err(BackendError::Disconnected(format!(
                "compositor withdrew {}",
                OrgKdeKwinDpmsManager::interface().name
            )));
        }
        pump.turn(&mut state).await?;
    }
}

impl PowerState {
    fn track(&mut self, global: OutputGlobal, output: &WlOutput, qh: &QueueHandle<Self>) {
        if let Some((_, manager)) = &self.manager {
            self.dpms.push(OutputDpms {
                global,
                proxy: manager.get(output, qh, global),
                pending: None,
                mode: None,
            });
        }
    }

    fn untrack(&mut self, global: OutputGlobal, name: Option<&str>) {
        if let Some(index) = self.dpms.iter().position(|d| d.global == global) {
            self.dpms.remove(index).proxy.release();
        }
        if let Some(name) = name {
            self.tracker.forget(name);
        }
    }

    /// Sends a `DisplayPower` event for every named output whose state
    /// changed since the last report.
    fn report(&mut self) {
        for dpms in &self.dpms {
            let Some(name) = self.outputs.name(dpms.global) else {
                continue;
            };
            let Some(on) = dpms.mode.and_then(power::is_on) else {
                continue;
            };
            if self.tracker.update(name, on) {
                tracing::debug!(output = name, on, "DPMS state");
                self.sink.send(Event::DisplayPower {
                    output: name.to_owned(),
                    on,
                    kind: PowerKind::Dpms,
                });
            }
        }
    }
}

impl AsMut<OutputRegistry> for PowerState {
    fn as_mut(&mut self) -> &mut OutputRegistry {
        &mut self.outputs
    }
}

impl Dispatch<WlRegistry, ()> for PowerState {
    fn event(
        state: &mut Self,
        registry: &WlRegistry,
        event: wl_registry::Event,
        (): &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match &event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } if interface == OrgKdeKwinDpmsManager::interface().name
                && state.manager.is_none() =>
            {
                let manager = registry.bind(*name, (*version).min(MANAGER_VERSION), qh, ());
                state.manager = Some((*name, manager));
                let bound: Vec<_> = state
                    .outputs
                    .bound()
                    .map(|(global, output)| (global, output.clone()))
                    .collect();
                for (global, output) in bound {
                    state.track(global, &output, qh);
                }
            }
            wl_registry::Event::GlobalRemove { name }
                if state
                    .manager
                    .as_ref()
                    .is_some_and(|(global, _)| global == name) =>
            {
                state.manager_removed = true;
            }
            _ => match state.outputs.handle_registry(registry, &event, qh) {
                Some(OutputChange::Added(global, output)) => state.track(global, &output, qh),
                Some(OutputChange::Removed(global, name)) => state.untrack(global, name.as_deref()),
                None => {}
            },
        }
    }
}

impl Dispatch<OrgKdeKwinDpms, OutputGlobal> for PowerState {
    fn event(
        state: &mut Self,
        _: &OrgKdeKwinDpms,
        event: org_kde_kwin_dpms::Event,
        global: &OutputGlobal,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(dpms) = state.dpms.iter_mut().find(|d| d.global == *global) else {
            return;
        };
        match event {
            org_kde_kwin_dpms::Event::Supported { supported } => {
                tracing::debug!(global = global.0, supported, "DPMS support");
            }
            org_kde_kwin_dpms::Event::Mode { mode } => dpms.pending = Some(mode),
            org_kde_kwin_dpms::Event::Done => {
                if let Some(mode) = dpms.pending.take() {
                    dpms.mode = Some(mode);
                }
                state.report();
            }
            _ => {}
        }
    }
}

delegate_dispatch!(PowerState: [WlOutput: OutputGlobal] => OutputRegistry);
delegate_dispatch!(PowerState: [WlCallback: Synced] => Synced);
delegate_noop!(PowerState: ignore OrgKdeKwinDpmsManager);
