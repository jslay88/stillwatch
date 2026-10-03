//! The idle notifier's requests and events on top of the shared fake
//! compositor.

use std::collections::HashMap;
use std::io;

pub use crate::wayland::test_server::FakeCompositor;
use crate::wayland::test_server::{Bind, REGISTRY_BIND};

const NOTIFIER_GET_INPUT_IDLE: u16 = 2;
const NOTIFICATION_IDLED: u16 = 0;
const NOTIFICATION_RESUMED: u16 = 1;

/// What the client asked for with `get_input_idle_notification`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Notification {
    pub id: u32,
    pub timeout_ms: u32,
    pub seat_is_bound_seat: bool,
}

pub trait IdleCompositor {
    /// Waits for both binds and the input idle notification request.
    fn accept_notification(&mut self) -> io::Result<Notification>;
    fn idled(&mut self, notification: Notification) -> io::Result<()>;
    fn resumed(&mut self, notification: Notification) -> io::Result<()>;
}

impl IdleCompositor for FakeCompositor {
    fn accept_notification(&mut self) -> io::Result<Notification> {
        let mut bound = HashMap::new();
        for _ in 0..2 {
            let bind = Bind::parse(self.expect(self.registry(), REGISTRY_BIND)?);
            bound.insert(bind.interface, bind.id);
        }
        let mut request = self.expect(bound["ext_idle_notifier_v1"], NOTIFIER_GET_INPUT_IDLE)?;
        Ok(Notification {
            id: request.uint(),
            timeout_ms: request.uint(),
            seat_is_bound_seat: request.uint() == bound["wl_seat"],
        })
    }

    fn idled(&mut self, notification: Notification) -> io::Result<()> {
        self.send(notification.id, NOTIFICATION_IDLED, &[])
    }

    fn resumed(&mut self, notification: Notification) -> io::Result<()> {
        self.send(notification.id, NOTIFICATION_RESUMED, &[])
    }
}
