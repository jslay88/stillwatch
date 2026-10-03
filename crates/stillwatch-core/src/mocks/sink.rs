use super::CallLog;
use crate::backend::EventSink;
use crate::event::Event;

/// An [`EventSink`] that keeps every event it receives.
#[derive(Debug, Default)]
pub struct RecordingSink {
    events: CallLog<Event>,
}

impl RecordingSink {
    /// An empty sink.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A copy of every event received, oldest first.
    #[must_use]
    pub fn events(&self) -> Vec<Event> {
        self.events.snapshot()
    }

    /// Removes and returns every event received.
    #[must_use]
    pub fn take(&self) -> Vec<Event> {
        self.events.take()
    }

    /// Number of events received.
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether nothing was received.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

impl EventSink for RecordingSink {
    fn send(&self, event: Event) {
        self.events.push(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::ControlCommand;

    #[test]
    fn records_events_in_order() {
        let sink = RecordingSink::new();
        assert!(sink.is_empty());
        sink.send(ControlCommand::Pause.into());
        sink.send(ControlCommand::Resume.into());
        assert_eq!(sink.len(), 2);
        assert_eq!(
            sink.events(),
            vec![ControlCommand::Pause.into(), ControlCommand::Resume.into()]
        );
        assert_eq!(sink.take().len(), 2);
        assert!(sink.is_empty());
    }
}
