//! Dispatch state for one connection: the outputs, the overlays on them, and
//! the rules for reporting overlays that appear or go away.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use smithay_client_toolkit::output::OutputState;
use smithay_client_toolkit::reexports::client::QueueHandle;
use smithay_client_toolkit::reexports::client::protocol::wl_output::WlOutput;
use smithay_client_toolkit::registry::RegistryState;
use stillwatch_core::backend::{BackendError, EventSink};
use stillwatch_core::event::Event;

use super::Shade;
use super::surface::{Overlay, Protocols};
use super::targets::Desired;

/// Everything the event handlers need.
pub struct OverlayState {
    /// SCTK's view of the registry.
    pub registry: RegistryState,
    /// Connected outputs and their names.
    pub outputs: OutputState,
    /// Globals overlays are made from.
    pub protocols: Protocols,
    /// What should be covered. Shared with the blanker so it survives
    /// reconnects.
    pub desired: Arc<Mutex<Desired>>,
    /// Where `DisplayPower` events go.
    pub sink: Arc<dyn EventSink>,
    /// One per covered (or about to be covered) output.
    pub overlays: Vec<Overlay>,
}

/// Locks `desired`, carrying on past a panicked holder: the value is plain
/// data and stays consistent.
pub fn lock(desired: &Mutex<Desired>) -> MutexGuard<'_, Desired> {
    desired.lock().unwrap_or_else(PoisonError::into_inner)
}

impl OverlayState {
    /// Connected outputs that have a connector name.
    pub fn present(&self) -> Vec<(String, WlOutput)> {
        self.outputs
            .outputs()
            .filter_map(|output| {
                let name = self.outputs.info(&output)?.name?;
                Some((name, output))
            })
            .collect()
    }

    /// The shade `name` should have right now.
    pub fn desired_shade(&self, name: &str) -> Option<Shade> {
        lock(&self.desired).shade_for(name)
    }

    /// Puts `shade` on `output`: a new overlay, or a redraw of the existing
    /// one.
    pub fn cover(
        &mut self,
        qh: &QueueHandle<Self>,
        wl_output: &WlOutput,
        name: &str,
        shade: Shade,
    ) {
        let Some(index) = self
            .overlays
            .iter()
            .position(|overlay| overlay.output == name)
        else {
            tracing::debug!(output = name, ?shade, "creating overlay");
            let overlay = Overlay::create(&self.protocols, qh, wl_output, name, shade);
            self.overlays.push(overlay);
            return;
        };
        if let Err(error) = self.overlays[index].set_shade(&self.protocols, qh, shade) {
            self.fail(index, &error);
        }
    }

    /// Shows the first (or a new) configure on overlay `index`.
    pub fn configured(&mut self, qh: &QueueHandle<Self>, index: usize, size: (u32, u32)) {
        let overlay = &self.overlays[index];
        let fallback = self
            .outputs
            .info(&overlay.wl_output)
            .and_then(|info| info.logical_size)
            .and_then(|(width, height)| {
                Some((u32::try_from(width).ok()?, u32::try_from(height).ok()?))
            });
        let was_covering = overlay.covering();
        match self.overlays[index].configure(&self.protocols, qh, size, fallback) {
            Ok(()) if !was_covering => {
                let overlay = &self.overlays[index];
                tracing::info!(output = %overlay.output, shade = ?overlay.shade(), "overlay showing");
                self.report(&overlay.output, false);
            }
            Ok(()) => {}
            Err(error) => self.fail(index, &error),
        }
    }

    /// Drops overlays that are no longer wanted and redraws ones whose shade
    /// changed.
    pub fn prune(&mut self, qh: &QueueHandle<Self>) {
        let desired = lock(&self.desired).clone();
        self.overlays.retain(|overlay| {
            let keep = desired.shade_for(&overlay.output).is_some();
            if !keep {
                tracing::info!(output = %overlay.output, "removing overlay");
            }
            keep
        });
        let mut index = 0;
        while index < self.overlays.len() {
            let wanted = desired.shade_for(&self.overlays[index].output);
            match wanted.map(|shade| self.overlays[index].set_shade(&self.protocols, qh, shade)) {
                Some(Err(error)) => self.fail(index, &error),
                _ => index += 1,
            }
        }
    }

    /// Whether every output in `names` is either covered with `shade` or has
    /// no overlay left (closed, or its output went away).
    pub fn settled(&self, names: &[String], shade: Shade) -> bool {
        names.iter().all(|name| {
            self.overlays
                .iter()
                .find(|overlay| &overlay.output == name)
                .is_none_or(|overlay| overlay.covering() && overlay.shade() == shade)
        })
    }

    /// Whether `name` is covered with `shade`.
    pub fn covered(&self, name: &str, shade: Shade) -> bool {
        self.overlays
            .iter()
            .any(|overlay| overlay.output == name && overlay.covering() && overlay.shade() == shade)
    }

    /// Removes overlay `index` because the compositor took it away, and
    /// reports the output as showing content again if it was covered.
    pub fn lose(&mut self, index: usize, why: &str) {
        let overlay = self.overlays.remove(index);
        tracing::warn!(output = %overlay.output, why, "overlay went away");
        if overlay.covering() {
            self.report(&overlay.output, true);
        }
    }

    /// Drops every overlay as the connection ends, reporting each covered
    /// output as showing content again.
    pub fn lose_all(&mut self) {
        for overlay in std::mem::take(&mut self.overlays) {
            if overlay.covering() {
                tracing::warn!(output = %overlay.output, "overlay lost with the connection");
                self.report(&overlay.output, true);
            }
        }
    }

    fn fail(&mut self, index: usize, error: &BackendError) {
        tracing::warn!(output = %self.overlays[index].output, %error, "can't draw the overlay");
        self.lose(index, "drawing failed");
    }

    fn report(&self, output: &str, on: bool) {
        self.sink.send(Event::DisplayPower {
            output: output.to_owned(),
            on,
        });
    }
}
