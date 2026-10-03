//! `wl_output` and `org_kde_kwin_dpms` on top of the shared fake compositor.

use std::collections::HashMap;
use std::io;
use std::os::unix::net::UnixStream;

use crate::wayland::test_server::{
    Arg, Bind, DISPLAY, DISPLAY_SYNC, FakeCompositor, REGISTRY_BIND,
};

pub const MANAGER: &str = "org_kde_kwin_dpms_manager";
pub const OUTPUT: &str = "wl_output";

pub const ON: u32 = 0;
pub const STANDBY: u32 = 1;
pub const OFF: u32 = 3;

const OUTPUT_RELEASE: u16 = 0;
const OUTPUT_MODE: u16 = 1;
const OUTPUT_DONE: u16 = 2;
const MODE_CURRENT: u32 = 1;
const OUTPUT_NAME: u16 = 4;
const MANAGER_GET: u16 = 0;
const DPMS_RELEASE: u16 = 1;
const DPMS_SUPPORTED: u16 = 0;
const DPMS_MODE: u16 = 1;
const DPMS_DONE: u16 = 2;

/// A compositor with outputs whose names and DPMS modes the test sets.
pub struct DpmsCompositor {
    pub fake: FakeCompositor,
    /// Connector name per output global. Outputs without one send no name.
    names: HashMap<u32, String>,
    /// Mode sent when a DPMS object is created, per output global.
    modes: HashMap<u32, u32>,
    /// Client object id of each bound output, to its global.
    outputs: HashMap<u32, u32>,
    manager: Option<u32>,
    /// DPMS object id per output global.
    pub dpms: HashMap<u32, u32>,
    /// Output globals whose DPMS object and `wl_output` were released.
    pub released_dpms: Vec<u32>,
    pub released_outputs: Vec<u32>,
    /// Versions the client bound each global at.
    pub versions: HashMap<u32, u32>,
}

impl DpmsCompositor {
    /// The compositor end, and the client's socket.
    pub fn pair(names: &[(u32, &str)]) -> (Self, UnixStream) {
        let (fake, client) = FakeCompositor::pair();
        let compositor = Self {
            fake,
            names: names.iter().map(|&(g, n)| (g, n.to_owned())).collect(),
            modes: HashMap::new(),
            outputs: HashMap::new(),
            manager: None,
            dpms: HashMap::new(),
            released_dpms: Vec::new(),
            released_outputs: Vec::new(),
            versions: HashMap::new(),
        };
        (compositor, client)
    }

    pub fn name(&mut self, global: u32, name: &str) {
        self.names.insert(global, name.to_owned());
    }

    /// The mode a DPMS object for `global` starts in. Default: on.
    pub fn starts(&mut self, global: u32, mode: u32) {
        self.modes.insert(global, mode);
    }

    /// Handles binds, DPMS `get`s, releases, and syncs until `done` holds.
    pub fn serve_until(&mut self, done: impl Fn(&Self) -> bool) -> io::Result<()> {
        while !done(self) {
            self.serve_one()?;
        }
        Ok(())
    }

    /// Serves until the client closes its end.
    pub fn serve_until_closed(&mut self) {
        while self.serve_one().is_ok() {}
    }

    fn serve_one(&mut self) -> io::Result<()> {
        let mut request = self.fake.read()?;
        match (request.object, request.opcode) {
            (DISPLAY, DISPLAY_SYNC) => {
                let callback = request.uint();
                self.fake.done(callback)?;
            }
            (object, REGISTRY_BIND) if object == self.fake.registry() => {
                self.bound(&Bind::parse(request))?;
            }
            (object, MANAGER_GET) if Some(object) == self.manager => {
                let id = request.uint();
                let global = self.outputs[&request.uint()];
                self.dpms.insert(global, id);
                let mode = self.modes.get(&global).copied().unwrap_or(ON);
                self.fake.send(id, DPMS_SUPPORTED, &[Arg::Uint(1)])?;
                self.mode(global, mode)?;
            }
            (object, DPMS_RELEASE) if self.dpms.values().any(|&id| id == object) => {
                let (&global, _) = self.dpms.iter().find(|&(_, &id)| id == object).unwrap();
                self.released_dpms.push(global);
            }
            (object, OUTPUT_RELEASE) if self.outputs.contains_key(&object) => {
                self.released_outputs.push(self.outputs[&object]);
            }
            _ => {}
        }
        Ok(())
    }

    /// Serves until `count` DPMS objects exist.
    pub fn serve_dpms(&mut self, count: usize) -> io::Result<()> {
        self.serve_until(|c| c.dpms.len() >= count)
    }

    fn bound(&mut self, bind: &Bind) -> io::Result<()> {
        self.versions.insert(bind.name, bind.version);
        match bind.interface.as_str() {
            MANAGER => self.manager = Some(bind.id),
            OUTPUT => {
                self.outputs.insert(bind.id, bind.name);
                if let Some(name) = self.names.get(&bind.name).cloned() {
                    self.fake.send(bind.id, OUTPUT_NAME, &[Arg::Str(&name)])?;
                }
                // Positive ints encode the same as uints on the wire.
                let mode = [MODE_CURRENT, 1920, 1080, 60_000].map(Arg::Uint);
                self.fake.send(bind.id, OUTPUT_MODE, &mode)?;
                self.fake.send(bind.id, OUTPUT_DONE, &[])?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Sends `mode` and `done` on the DPMS object of output `global`.
    pub fn mode(&mut self, global: u32, mode: u32) -> io::Result<()> {
        let id = self.dpms[&global];
        self.fake.send(id, DPMS_MODE, &[Arg::Uint(mode)])?;
        self.fake.send(id, DPMS_DONE, &[])
    }
}
