//! `wl_output` add and remove, including across a compositor restart.
//!
//! Each connection gets an epoch from the supervisor. A new `wl_output`
//! global takes the next local id, so a connector name that comes back after
//! the compositor restarts does not keep the previous generation.

use std::collections::HashMap;
use std::sync::Arc;

use stillwatch_core::backend::{BackendError, EventSink};
use stillwatch_core::event::Event;
use stillwatch_core::luma::OutputInfo;
use wayland_client::protocol::{
    wl_callback::WlCallback,
    wl_output::WlOutput,
    wl_registry::{self, WlRegistry},
};
use wayland_client::{Connection, Dispatch, QueueHandle, delegate_dispatch};

use crate::wayland::outputs::{OutputGlobal, OutputRegistry};
use crate::wayland::{EventPump, Synced};

/// Watches `WAYLAND_DISPLAY` until the connection fails.
///
/// `epoch` is added into every generation so the next attempt after a
/// restart cannot reuse the previous connection's ids.
///
/// # Errors
///
/// [`BackendError::Disconnected`] when the compositor is gone.
pub(crate) async fn watch(epoch: u64, sink: Arc<dyn EventSink>) -> Result<(), BackendError> {
    let conn = crate::wayland::connect_to_env()?;
    watch_on(&conn, epoch, sink).await
}

/// Watches `conn` until it fails.
///
/// # Errors
///
/// [`BackendError::Disconnected`] when the connection ends.
pub(crate) async fn watch_on(
    conn: &Connection,
    epoch: u64,
    sink: Arc<dyn EventSink>,
) -> Result<(), BackendError> {
    let mut pump = EventPump::new(conn)?;
    let qh = pump.handle();
    let mut state = State {
        outputs: OutputRegistry::default(),
        generations: HashMap::new(),
        next_local: 0,
        epoch,
        last: None,
        sink,
    };
    let _registry = conn.display().get_registry(&qh, ());
    // Globals arrive in the first round. Binding them is what makes the
    // compositor send names and modes, and those follow the next sync.
    pump.roundtrip(conn, &mut state).await?;
    pump.roundtrip(conn, &mut state).await?;
    loop {
        state.publish();
        pump.turn(&mut state).await?;
    }
}

struct State {
    outputs: OutputRegistry,
    generations: HashMap<u32, u64>,
    next_local: u64,
    epoch: u64,
    last: Option<Vec<OutputInfo>>,
    sink: Arc<dyn EventSink>,
}

impl State {
    fn publish(&mut self) {
        let live: Vec<u32> = self.outputs.bound().map(|(global, _)| global.0).collect();
        self.generations.retain(|id, _| live.contains(id));
        let mut outputs: Vec<OutputInfo> = self
            .outputs
            .named()
            .into_iter()
            .map(|output| {
                let generation = self.generation(output.global.0);
                OutputInfo::new(output.name, output.width, output.height)
                    .with_generation(generation)
            })
            .collect();
        outputs.sort_by(|left, right| left.name.cmp(&right.name));
        if self.last.as_ref() == Some(&outputs) {
            return;
        }
        self.last = Some(outputs.clone());
        self.sink.send(Event::OutputsChanged(outputs));
    }

    fn generation(&mut self, global: u32) -> u64 {
        if let Some(generation) = self.generations.get(&global) {
            return *generation;
        }
        let local = self.next_local;
        self.next_local = self.next_local.saturating_add(1);
        let generation = self.epoch.saturating_mul(1_000_000).saturating_add(local);
        self.generations.insert(global, generation);
        generation
    }
}

impl AsMut<OutputRegistry> for State {
    fn as_mut(&mut self) -> &mut OutputRegistry {
        &mut self.outputs
    }
}

impl Dispatch<WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &WlRegistry,
        event: wl_registry::Event,
        (): &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        state.outputs.handle_registry(registry, &event, qh);
    }
}

delegate_dispatch!(State: [WlOutput: OutputGlobal] => OutputRegistry);
delegate_dispatch!(State: [WlCallback: Synced] => Synced);

#[cfg(test)]
mod tests;
