//! A scripted [`DdcTransport`]: displays that remember their power mode,
//! with queued failures, delays, and a call log.

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use super::DdcError;
use super::transport::{DdcDisplay, DdcTransport, Scan};

/// A call the transport received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Call {
    Scan,
    Get(String, u8),
    Set(String, u8, u16),
}

#[derive(Debug, Default)]
pub(crate) struct FakeBus {
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    displays: Vec<DdcDisplay>,
    denied: Vec<String>,
    scan_error: Option<DdcError>,
    power: HashMap<String, u16>,
    failures: VecDeque<String>,
    silent_in_standby: bool,
    delay: Duration,
    panic_next: bool,
    calls: Vec<Call>,
}

impl FakeBus {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Adds a display that starts powered on.
    pub(crate) fn with_display(self, id: &str, edid: &[u8]) -> Self {
        {
            let mut state = self.lock();
            state.displays.push(DdcDisplay {
                id: id.into(),
                edid: edid.to_vec(),
            });
            state.power.insert(id.into(), 0x01);
        }
        self
    }

    /// Makes scans report `node` as permission denied.
    pub(crate) fn with_denied(self, node: &str) -> Self {
        self.lock().denied.push(node.into());
        self
    }

    pub(crate) fn fail_scans(&self, error: DdcError) {
        self.lock().scan_error = Some(error);
    }

    /// Makes the next `count` reads or writes fail.
    pub(crate) fn fail_next(&self, count: usize, detail: &str) {
        let mut state = self.lock();
        state
            .failures
            .extend(std::iter::repeat_n(detail.to_owned(), count));
    }

    /// Makes every read and write take `delay` of real time.
    pub(crate) fn set_delay(&self, delay: Duration) {
        self.lock().delay = delay;
    }

    pub(crate) fn panic_next(&self) {
        self.lock().panic_next = true;
    }

    /// Displays not powered on don't answer reads, like many in standby.
    pub(crate) fn silent_in_standby(&self) {
        self.lock().silent_in_standby = true;
    }

    /// Changes a display's power mode behind the blanker's back, like its
    /// power button.
    pub(crate) fn set_power(&self, id: &str, value: u16) {
        self.lock().power.insert(id.into(), value);
    }

    pub(crate) fn power(&self, id: &str) -> Option<u16> {
        self.lock().power.get(id).copied()
    }

    pub(crate) fn calls(&self) -> Vec<Call> {
        self.lock().calls.clone()
    }

    pub(crate) fn writes(&self) -> Vec<(String, u16)> {
        self.calls()
            .into_iter()
            .filter_map(|call| match call {
                Call::Set(id, _, value) => Some((id, value)),
                _ => None,
            })
            .collect()
    }

    pub(crate) fn reads(&self) -> usize {
        self.calls()
            .iter()
            .filter(|call| matches!(call, Call::Get(..)))
            .count()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap()
    }

    /// Logs `call`, then waits and fails as scripted.
    fn begin(&self, call: Call) -> Result<(), String> {
        let (delay, panic, failure) = {
            let mut state = self.lock();
            state.calls.push(call);
            let panic = std::mem::take(&mut state.panic_next);
            (state.delay, panic, state.failures.pop_front())
        };
        assert!(!panic, "scripted transport panic");
        std::thread::sleep(delay);
        failure.map_or(Ok(()), Err)
    }
}

impl DdcTransport for FakeBus {
    fn scan(&self) -> Result<Scan, DdcError> {
        let mut state = self.lock();
        state.calls.push(Call::Scan);
        if let Some(error) = state.scan_error.clone() {
            return Err(error);
        }
        Ok(Scan {
            displays: state.displays.clone(),
            denied: state.denied.clone(),
        })
    }

    fn get_vcp(&self, display: &str, code: u8) -> Result<u16, String> {
        self.begin(Call::Get(display.into(), code))?;
        let state = self.lock();
        let value = *state
            .power
            .get(display)
            .ok_or_else(|| format!("no display {display}"))?;
        if state.silent_in_standby && value != 0x01 {
            return Err("no reply".into());
        }
        Ok(value)
    }

    fn set_vcp(&self, display: &str, code: u8, value: u16) -> Result<(), String> {
        self.begin(Call::Set(display.into(), code, value))?;
        let mut state = self.lock();
        let power = state
            .power
            .get_mut(display)
            .ok_or_else(|| format!("no display {display}"))?;
        *power = value;
        Ok(())
    }
}
