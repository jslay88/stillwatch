use std::io;
use std::path::Path;

use evdev::{Device, EventStream, EventSummary, InputEvent};

use super::axis::{Axes, AxisRange, PadInput};
use super::platform::Pad;

/// An evdev event node read through tokio, so an idle pad costs nothing.
pub(crate) struct EvdevPad {
    name: String,
    axes: Axes,
    stream: EventStream,
}

impl EvdevPad {
    /// Opens `node` and captures its axis ranges. Needs a tokio runtime.
    pub(crate) fn open(node: &Path) -> io::Result<Self> {
        let device = Device::open(node)?;
        let name = device.name().unwrap_or("Unknown gamepad").to_owned();
        let axes = Axes::new(device.get_absinfo()?.map(|(code, info)| {
            (
                code.0,
                AxisRange {
                    min: info.minimum(),
                    max: info.maximum(),
                    flat: info.flat(),
                    value: info.value(),
                },
            )
        }));
        let stream = device.into_event_stream()?;
        Ok(Self { name, axes, stream })
    }
}

fn classify(event: InputEvent) -> PadInput {
    match event.destructure() {
        EventSummary::Key(..) => PadInput::Button,
        EventSummary::RelativeAxis(..) => PadInput::Relative,
        EventSummary::AbsoluteAxis(_, code, value) => PadInput::Absolute {
            code: code.0,
            value,
        },
        _ => PadInput::Other,
    }
}

impl Pad for EvdevPad {
    fn name(&self) -> &str {
        &self.name
    }

    fn axes(&self) -> Axes {
        self.axes.clone()
    }

    async fn next_input(&mut self) -> io::Result<PadInput> {
        self.stream.next_event().await.map(classify)
    }
}

#[cfg(test)]
mod tests {
    use evdev::{AbsoluteAxisCode, EventType, KeyCode, RelativeAxisCode, SynchronizationCode};

    use super::*;

    #[test]
    fn classifies_event_types() {
        let key = InputEvent::new(EventType::KEY.0, KeyCode::BTN_SOUTH.0, 1);
        assert_eq!(classify(key), PadInput::Button);
        let rel = InputEvent::new(EventType::RELATIVE.0, RelativeAxisCode::REL_X.0, -3);
        assert_eq!(classify(rel), PadInput::Relative);
        let abs = InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_RY.0, -900);
        assert_eq!(
            classify(abs),
            PadInput::Absolute {
                code: AbsoluteAxisCode::ABS_RY.0,
                value: -900
            }
        );
        let syn = InputEvent::new(
            EventType::SYNCHRONIZATION.0,
            SynchronizationCode::SYN_REPORT.0,
            0,
        );
        assert_eq!(classify(syn), PadInput::Other);
        let misc = InputEvent::new(EventType::MISC.0, 4, 90001);
        assert_eq!(classify(misc), PadInput::Other);
    }

    #[test]
    fn opening_a_missing_node_fails_with_not_found() {
        let err = EvdevPad::open(Path::new("/dev/input/event-stillwatch-missing"))
            .err()
            .unwrap();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn opening_a_non_evdev_file_fails() {
        assert!(EvdevPad::open(Path::new("/dev/null")).is_err());
    }
}
