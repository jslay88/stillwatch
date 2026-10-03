use std::time::Duration;

use stillwatch_core::backend::{BackendError, GamepadDevice};
use stillwatch_core::event::{ActivityEvent, Event};
use stillwatch_core::mocks::{MockGamepadSource, MockIdleSource, WatchEnd};

use super::*;

const PAD: &str = "/dev/input/event7";

fn pads_named(name: &str) -> MockGamepadSource {
    let pads = MockGamepadSource::new();
    pads.set_devices(vec![GamepadDevice {
        id: PAD.into(),
        name: name.into(),
        ignored: false,
        last_activity: None,
    }]);
    pads
}

fn pad_event() -> Event {
    ActivityEvent::GamepadActivity { device: PAD.into() }.into()
}

fn one_minute() -> ActivitySettings {
    ActivitySettings {
        input_idle: Duration::from_mins(1),
        gamepad: true,
    }
}

async fn after(span: Duration) {
    tokio::time::sleep(span).await;
}

/// What each printed line says after the timestamp.
fn transitions(out: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(out)
        .lines()
        .skip(1)
        .map(|line| line.split_once("  ").unwrap().1.to_owned())
        .collect()
}

#[tokio::test(start_paused = true)]
async fn prints_idle_and_gamepad_wakes_until_shutdown() {
    let idle = MockIdleSource::new();
    idle.push_run(vec![ActivityEvent::InputIdle.into()], WatchEnd::Hang);
    let pads = pads_named("Xbox Wireless Controller");
    // The first pad watch fails, so the pad event lands a back-off (1s)
    // after compositor idle.
    let lost = BackendError::Disconnected("udev".into());
    pads.push_run(Vec::new(), WatchEnd::Fail(lost));
    pads.push_run(vec![pad_event()], WatchEnd::Hang);
    let sources = ActivitySources {
        idle: &idle,
        gamepad: Some(&pads),
    };

    let mut out = Vec::new();
    let shutdown = after(Duration::from_mins(30));
    watch(sources, one_minute(), &TokioClock, &mut out, shutdown)
        .await
        .unwrap();

    let text = String::from_utf8_lossy(&out);
    assert_eq!(
        text.lines().next(),
        Some("Watching keyboard, mouse, and gamepads with a 1m idle timeout. Ctrl-C to stop.")
    );
    assert_eq!(
        transitions(&out),
        ["idle", "active (gamepad: Xbox Wireless Controller)", "idle"]
    );
    assert_eq!(idle.timeouts(), [Duration::from_mins(1)]);
}

#[tokio::test(start_paused = true)]
async fn a_compositor_without_input_idle_is_an_error() {
    let idle = MockIdleSource::new();
    let v1 = BackendError::Unsupported("ext-idle-notify v1 only".into());
    idle.push_run(Vec::new(), WatchEnd::Fail(v1));
    let sources = ActivitySources {
        idle: &idle,
        gamepad: None,
    };
    let mut out = Vec::new();
    let shutdown = after(Duration::from_mins(30));
    let err = watch(sources, one_minute(), &TokioClock, &mut out, shutdown)
        .await
        .unwrap_err();
    assert_eq!(err.to_string(), "input idle stopped");
    assert!(
        String::from_utf8_lossy(&out).contains("keyboard and mouse (gamepads are off)"),
        "{}",
        String::from_utf8_lossy(&out)
    );
}

struct Closed;

impl Write for Closed {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::ErrorKind::BrokenPipe.into())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[tokio::test(start_paused = true)]
async fn a_closed_stdout_stops_it() {
    let idle = MockIdleSource::new();
    let sources = ActivitySources {
        idle: &idle,
        gamepad: None,
    };
    let shutdown = after(Duration::from_mins(30));
    let err = watch(sources, one_minute(), &TokioClock, &mut Closed, shutdown)
        .await
        .unwrap_err();
    assert_eq!(
        err.downcast_ref::<io::Error>().map(io::Error::kind),
        Some(io::ErrorKind::BrokenPipe)
    );
}

#[test]
fn lines_name_the_wake_source() {
    let pads = pads_named("DualSense");
    let at = Timestamp::UNIX_EPOCH;
    let cases = [
        (Presence::Idle, "idle"),
        (
            Presence::Active(WakeSource::KeyboardMouse),
            "active (keyboard/mouse)",
        ),
        (
            Presence::Active(WakeSource::Gamepad { device: PAD.into() }),
            "active (gamepad: DualSense)",
        ),
        (
            Presence::Active(WakeSource::Gamepad {
                device: "/dev/input/event9".into(),
            }),
            "active (gamepad: /dev/input/event9)",
        ),
        (
            Presence::Active(WakeSource::WatchRestarted),
            "active (idle watch restarted)",
        ),
    ];
    for (presence, expected) in cases {
        let line = line(at, &presence, Some(&pads));
        assert!(line.ends_with(&format!("  {expected}")), "{line}");
        assert_eq!(line.len(), "1970-01-01 00:00:00  ".len() + expected.len());
    }
    let unplugged = Presence::Active(WakeSource::Gamepad { device: PAD.into() });
    assert!(line(at, &unplugged, None).ends_with("(gamepad: /dev/input/event7)"));
}
