//! End to end against the real udev monitor and evdev, using a uinput virtual
//! gamepad. Skips itself where `/dev/uinput` isn't writable (most CI).

use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
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
use tokio::task::JoinHandle;
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

/// Shaped like a keyboard's "System Control" interface: a few keys, two hats,
/// and `ABS_MISC`. udev tags it `ID_INPUT_JOYSTICK`. Never emits anything.
fn virtual_system_control(name: &str) -> io::Result<VirtualDevice> {
    let mut keys = AttributeSet::<KeyCode>::new();
    keys.insert(KeyCode::KEY_MENU);
    keys.insert(KeyCode::KEY_PROG1);
    let hat = AbsInfo::new(0, -1, 1, 0, 0, 0);
    let misc = AbsInfo::new(0, 0, 255, 0, 0, 0);
    VirtualDevice::builder()?
        .name(name)
        .with_keys(&keys)?
        .with_absolute_axis(&UinputAbsSetup::new(AbsoluteAxisCode::ABS_HAT0X, hat))?
        .with_absolute_axis(&UinputAbsSetup::new(AbsoluteAxisCode::ABS_HAT0Y, hat))?
        .with_absolute_axis(&UinputAbsSetup::new(AbsoluteAxisCode::ABS_MISC, misc))?
        .build()
}

fn uinput_writable() -> bool {
    let writable = OpenOptions::new().write(true).open("/dev/uinput").is_ok();
    if !writable {
        eprintln!("skipping: /dev/uinput isn't writable");
    }
    writable
}

fn start(source: &Arc<EvdevGamepadSource>) -> (JoinHandle<()>, mpsc::UnboundedReceiver<Event>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let source = Arc::clone(source);
    let watching = tokio::spawn(async move {
        let sink = move |event: Event| {
            let _ = tx.send(event);
        };
        let _ = source.watch(Arc::new(sink)).await;
    });
    (watching, rx)
}

/// Whether udev has tagged `node` as a joystick and we can read it, so the
/// source would open it.
fn openable_joystick(node: &Path) -> bool {
    let Some(name) = node.file_name() else {
        return false;
    };
    let tagged = tokio_udev::Device::from_syspath(&Path::new("/sys/class/input").join(name))
        .is_ok_and(|device| {
            device.is_initialized()
                && device
                    .property_value("ID_INPUT_JOYSTICK")
                    .is_some_and(|value| value == "1")
        });
    tagged && File::open(node).is_ok()
}

/// The device's event node, once the source would open it.
async fn openable_event_node(device: &mut VirtualDevice) -> Option<PathBuf> {
    let node = device
        .enumerate_dev_nodes_blocking()
        .ok()?
        .filter_map(Result::ok)
        .find(|node| {
            node.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("event"))
        })?;
    timeout(WAIT, async {
        while !openable_joystick(&node) {
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .ok()?;
    Some(node)
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
    if !uinput_writable() {
        return;
    }
    let name = format!("Stillwatch Test Pad {}", std::process::id());
    let source = Arc::new(EvdevGamepadSource::new(&GamepadSettings::default()));
    let (watching, mut rx) = start(&source);

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

#[tokio::test(flavor = "multi_thread")]
async fn a_tagged_node_without_gamepad_buttons_is_never_a_device() {
    if !uinput_writable() {
        return;
    }
    let id = std::process::id();
    let mut keyboard =
        virtual_system_control(&format!("Stillwatch Test System Control {id}")).unwrap();
    let keyboard_node = openable_event_node(&mut keyboard)
        .await
        .unwrap()
        .display()
        .to_string();

    let source = Arc::new(EvdevGamepadSource::new(&GamepadSettings::default()));
    let (watching, _rx) = start(&source);
    let name = format!("Stillwatch Test Flight Stick {id}");
    let pad = virtual_pad(&name).unwrap();
    wait_for_device(&source, &name, true)
        .await
        .unwrap()
        .unwrap();
    assert!(
        source
            .devices()
            .iter()
            .all(|device| device.id != keyboard_node)
    );

    drop(pad);
    drop(keyboard);
    watching.abort();
}
