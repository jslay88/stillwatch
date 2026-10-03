use crate::event::Event;

/// Where backends deliver events.
///
/// `send` must not block for long; implementations typically push into an
/// unbounded channel. Any `Fn(Event) + Send + Sync` closure is a sink, so the
/// daemon can wrap its channel sender without a newtype.
pub trait EventSink: Send + Sync {
    /// Delivers one event. Events sent after the receiver is gone are dropped.
    fn send(&self, event: Event);
}

impl<F> EventSink for F
where
    F: Fn(Event) + Send + Sync,
{
    fn send(&self, event: Event) {
        self(event);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, mpsc};

    use super::*;
    use crate::event::ActivityEvent;

    #[test]
    fn closures_are_sinks() {
        let (tx, rx) = mpsc::channel();
        let sink: Arc<dyn EventSink> = Arc::new(move |event| {
            let _ = tx.send(event);
        });
        sink.send(ActivityEvent::InputIdle.into());
        assert_eq!(
            rx.recv().unwrap(),
            Event::Activity(ActivityEvent::InputIdle)
        );
    }
}
