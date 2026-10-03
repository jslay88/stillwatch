//! End to end against the real udev monitor and evdev, using a uinput virtual
//! gamepad. Skips itself where `/dev/uinput` isn't writable (most CI).

use std::fs::OpenOptions;
use std::io;
use std::sync::Arc;
use std::time::Duration;

use evdev::uinput::VirtualDevice;
use evdev::{
    AbsInfo, AbsoluteAxisCode, AttributeSet, EventType, InputEvent, KeyCode, UinputAbsSetup,
};
use stillwatch_core::backend::{GamepadDevice, GamepadSource};
use stillwatch_core::event::{ActivityEvent, Event};
use stillwatchd::gamepad::{ACTIVITY_INTERVAL, EvdevGamepadSource, GamepadSettings};
use tokio::sync::mpsc;
use tokio::time::{error::Elapsed, sleep, timeout};

const WAIT: Duration = Duration::from_secs(10);
const QUIET: Duration = Duration::from_millis(300);

fn virtual_pad(name: &str) -> io::Result<VirtualDevice> {
    let mut keys = AttributeSet::<KeyCode>::new();
    keys.insert(KeyCode::BTN_SOUTH);
    keys.insert(KeyCode::BTN_EAST);
    let stick = AbsInfo::new(0, -32768, 32767, 16, 128, 0);
    VirtualDevice::builder()?
        .name(name)
        .with_keys(&keys)?
        .with_absolute_axis(&UinputAbsSetup::new(AbsoluteAxisCode::ABS_X, stick))?
        .with_absolute_axis(&UinputAbsSetup::new(AbsoluteAxisCode::ABS_Y, stick))?
        .build()
}

fn find(source: &EvdevGamepadSource, name: &str) -> Option<GamepadDevice> {
    source
        .devices()
        .into_iter()
        .find(|device| device.name == name)
}

async fn wait_for_device(
    source: &EvdevGamepadSource,
    name: &str,
    present: bool,
) -> Result<Option<GamepadDevice>, Elapsed> {
    timeout(WAIT, async {
        loop {
            let device = find(source, name);
            if device.is_some() == present {
                return device;
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
}

async fn activity_from(
    rx: &mut mpsc::UnboundedReceiver<Event>,
    id: &str,
    within: Duration,
) -> bool {
    let wanted = Event::Activity(ActivityEvent::GamepadActivity { device: id.into() });
    timeout(within, async {
        while let Some(event) = rx.recv().await {
            if event == wanted {
                return;
            }
        }
    })
    .await
    .is_ok()
}

fn press(pad: &mut VirtualDevice, key: KeyCode) -> io::Result<()> {
    pad.emit(&[InputEvent::new(EventType::KEY.0, key.0, 1)])?;
    pad.emit(&[InputEvent::new(EventType::KEY.0, key.0, 0)])
}

fn move_stick(pad: &mut VirtualDevice, value: i32) -> io::Result<()> {
    let event = InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_X.0, value);
    pad.emit(&[event])
}

#[tokio::test(flavor = "multi_thread")]
async fn virtual_gamepad_hotplug_deadzone_and_ignore_list() {
    if OpenOptions::new().write(true).open("/dev/uinput").is_err() {
        eprintln!("skipping: /dev/uinput isn't writable");
        return;
    }
    let name = format!("Stillwatch Test Pad {}", std::process::id());
    let source = Arc::new(EvdevGamepadSource::new(&GamepadSettings::default()));
    let (tx, mut rx) = mpsc::unbounded_channel();
    let watching = {
        let source = Arc::clone(&source);
        tokio::spawn(async move {
            let sink = move |event: Event| {
                let _ = tx.send(event);
            };
            source.watch(Arc::new(sink)).await
        })
    };

    let mut pad = virtual_pad(&name).unwrap();
    let device = wait_for_device(&source, &name, true)
        .await
        .unwrap()
        .unwrap();
    assert!(device.id.starts_with("/dev/input/event"));
    assert!(!device.ignored);

    move_stick(&mut pad, 1000).unwrap();
    assert!(!activity_from(&mut rx, &device.id, QUIET).await);
    assert_eq!(find(&source, &name).unwrap().last_activity, None);

    move_stick(&mut pad, 30_000).unwrap();
    assert!(activity_from(&mut rx, &device.id, WAIT).await);

    source.update_settings(&GamepadSettings {
        ignore_devices: vec!["stillwatch test pad".into()],
        ..GamepadSettings::default()
    });
    sleep(ACTIVITY_INTERVAL).await;
    press(&mut pad, KeyCode::BTN_SOUTH).unwrap();
    assert!(!activity_from(&mut rx, &device.id, QUIET).await);
    let ignored = find(&source, &name).unwrap();
    assert!(ignored.ignored);
    assert!(ignored.last_activity.is_some());

    source.update_settings(&GamepadSettings::default());
    press(&mut pad, KeyCode::BTN_EAST).unwrap();
    assert!(activity_from(&mut rx, &device.id, WAIT).await);

    drop(pad);
    assert_eq!(wait_for_device(&source, &name, false).await.unwrap(), None);
    assert!(!watching.is_finished());
    watching.abort();
}
