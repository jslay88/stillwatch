//! Output lists and a lost compositor idle watch.

use super::Ctx;
use crate::history::HistoryKind;
use crate::luma::OutputInfo;
use crate::time::TimerId;

impl Ctx {
    /// Starts capture again after outputs return, when a watch is already running.
    pub(in crate::state) fn resume_capture(&mut self) {
        if !self.activity_known() || self.captures_paused() || self.is_armed(TimerId::Capture) {
            return;
        }
        self.request_capture();
        self.arm_capture();
    }

    /// The idle watch is down. Activity is unknown: not idle, and not input.
    pub(in crate::state) fn lost_compositor(&mut self) {
        self.compositor = super::Compositor::Down;
        self.idle = false;
        self.disarm(TimerId::Capture);
        self.disarm(TimerId::ReblankGrace);
        self.disarm(TimerId::LockedBlank);
        self.reconnects = self.reconnects.saturating_add(1);
        let entry = self
            .history(HistoryKind::Reconnect)
            .with_count(self.reconnects);
        self.emit(crate::command::Command::Record(entry));
    }

    /// Stores the connected outputs. The first report is the baseline and is
    /// not history. Later adds and removals, including a reused connector
    /// name, are recorded as names and a count only.
    pub(in crate::state) fn note_outputs(&mut self, outputs: &[OutputInfo]) {
        let previous = self.outputs.replace(outputs.to_vec());
        if let Some(previous) = previous {
            self.record_hotplug(&previous, outputs);
        }
        if self.captures_paused() {
            self.disarm(TimerId::Capture);
            self.disarm(TimerId::ReblankGrace);
        }
    }

    /// True once outputs are known and none of the monitored ones are connected.
    pub(in crate::state) fn captures_paused(&self) -> bool {
        self.outputs.is_some() && !self.monitored_present()
    }

    /// Remembers the generations present when a blank starts.
    pub(in crate::state) fn snapshot_blank(&mut self) {
        self.blank_generation = self
            .outputs
            .iter()
            .flatten()
            .map(|output| (output.name.clone(), output.generation))
            .collect();
        self.hotplug_wakes.clear();
    }

    /// Whether `output` reporting power on should arm the re-blank watchdog.
    ///
    /// A generation that differs from the one snapshotted at blank time counts
    /// at most once. A generation that matches counts every time, which is a
    /// display that came back on without being replaced.
    pub(in crate::state) fn reconnect_wake(&mut self, output: &str) -> bool {
        if !self.activity_known() || self.captures_paused() {
            return false;
        }
        let Some(connected) = self.outputs.as_ref() else {
            return true;
        };
        let Some(current) = connected
            .iter()
            .find(|info| info.name == output)
            .map(|info| info.generation)
        else {
            return false;
        };
        if self.blank_generation.get(output).copied() == Some(current) {
            return true;
        }
        if self.hotplug_wakes.get(output).copied() == Some(current) {
            return false;
        }
        self.hotplug_wakes.insert(output.to_owned(), current);
        true
    }

    fn monitored_present(&self) -> bool {
        let Some(outputs) = self.outputs.as_ref() else {
            return false;
        };
        let monitored = &self.config.stale.monitored_outputs;
        outputs
            .iter()
            .any(|output| monitored.is_empty() || monitored.iter().any(|name| name == &output.name))
    }

    fn record_hotplug(&mut self, previous: &[OutputInfo], outputs: &[OutputInfo]) {
        let same = |left: &OutputInfo, right: &OutputInfo| {
            left.name == right.name && left.generation == right.generation
        };
        for output in previous {
            if !outputs.iter().any(|next| same(output, next)) {
                let entry = self
                    .history(HistoryKind::Hotplug)
                    .with_output(&output.name)
                    .with_count(0);
                self.emit(crate::command::Command::Record(entry));
            }
        }
        for output in outputs {
            if !previous.iter().any(|prev| same(output, prev)) {
                let entry = self
                    .history(HistoryKind::Hotplug)
                    .with_output(&output.name)
                    .with_count(1);
                self.emit(crate::command::Command::Record(entry));
            }
        }
    }
}
