//! One connection's worth of input idle watching: read the registry, bind,
//! create the notification, and forward its events until the connection
//! ends.

use std::sync::Arc;
use std::time::Duration;

use stillwatch_core::backend::{BackendError, EventSink};
use wayland_client::globals::Global;
use wayland_client::protocol::wl_callback::{self, WlCallback};
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{Connection, Dispatch, QueueHandle, delegate_noop};
use wayland_protocols::ext::idle_notify::v1::client::ext_idle_notification_v1::{
    self, ExtIdleNotificationV1,
};
use wayland_protocols::ext::idle_notify::v1::client::ext_idle_notifier_v1::ExtIdleNotifierV1;

use super::protocol;
use crate::wayland::EventPump;

/// Dispatch state for one connection.
struct IdleState {
    sink: Arc<dyn EventSink>,
    globals: Vec<Global>,
    synced: bool,
}

/// Watches input idle on `conn` until the connection fails.
///
/// Never returns `Ok`: a healthy connection keeps reporting forever.
pub async fn watch_on(
    conn: &Connection,
    timeout: Duration,
    sink: Arc<dyn EventSink>,
) -> Result<(), BackendError> {
    let mut pump = EventPump::new(conn)?;
    let qh = pump.handle();
    let mut state = IdleState {
        sink,
        globals: Vec::new(),
        synced: false,
    };

    let display = conn.display();
    let registry = display.get_registry(&qh, ());
    display.sync(&qh, ());
    pump.run_until(&mut state, |state| state.synced).await?;

    let binding = protocol::negotiate(&state.globals)?;
    let seat: WlSeat = registry.bind(binding.seat.name, binding.seat.version, &qh, ());
    let notifier: ExtIdleNotifierV1 =
        registry.bind(binding.notifier.name, binding.notifier.version, &qh, ());
    let timeout_ms = protocol::timeout_ms(timeout);
    let _notification = notifier.get_input_idle_notification(timeout_ms, &seat, &qh, ());
    tracing::info!(
        notifier_version = binding.notifier.version,
        timeout_ms,
        "watching input idle"
    );

    loop {
        pump.turn(&mut state).await?;
    }
}

impl Dispatch<WlRegistry, ()> for IdleState {
    fn event(
        state: &mut Self,
        _: &WlRegistry,
        event: wl_registry::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => state.globals.push(Global {
                name,
                interface,
                version,
            }),
            wl_registry::Event::GlobalRemove { name } => {
                state.globals.retain(|global| global.name != name);
            }
            _ => {}
        }
    }
}

impl Dispatch<WlCallback, ()> for IdleState {
    fn event(
        state: &mut Self,
        _: &WlCallback,
        event: wl_callback::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_callback::Event::Done { .. } = event {
            state.synced = true;
        }
    }
}

impl Dispatch<ExtIdleNotificationV1, ()> for IdleState {
    fn event(
        state: &mut Self,
        _: &ExtIdleNotificationV1,
        event: ext_idle_notification_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let Some(activity) = protocol::activity(&event) {
            tracing::debug!(?activity, "input idle event");
            state.sink.send(activity.into());
        }
    }
}

delegate_noop!(IdleState: ignore WlSeat);
delegate_noop!(IdleState: ignore ExtIdleNotifierV1);
