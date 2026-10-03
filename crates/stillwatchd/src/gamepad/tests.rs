use std::collections::{HashMap, VecDeque};
use std::future::{self, Future};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use evdev::{AbsoluteAxisCode as Abs, KeyCode as Key};
use stillwatch_core::backend::{BackendError, GamepadSource};
use stillwatch_core::event::{ActivityEvent, Event};
use stillwatch_core::mocks::RecordingSink;
use stillwatch_core::time::{Clock as _, FakeClock};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use super::axis::{Axes, AxisRange, PadInput};
use super::platform::{Hotplug, HotplugReceiver, Pad, Platform};
use super::{ACTIVITY_INTERVAL, EvdevGamepadSource, GamepadSettings, Watcher};

const PAD: &str = "/dev/input/event20";
const OTHER: &str = "/dev/input/event21";
const KEYBOARD: &str = "/dev/input/event17";

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap()
}

struct FakePad {
    name: String,
    keys: Vec<u16>,
    axes: Axes,
    inputs: mpsc::UnboundedReceiver<io::Result<PadInput>>,
    polls: watch::Sender<usize>,
}

impl Pad for FakePad {
    fn name(&self) -> &str {
        &self.name
    }

    fn keys(&self) -> &[u16] {
        &self.keys
    }

    fn axes(&self) -> Axes {
        self.axes.clone()
    }

    async fn next_input(&mut self) -> io::Result<PadInput> {
        self.polls.send_modify(|polls| *polls += 1);
        self.inputs
            .recv()
            .await
            .unwrap_or_else(|| Err(io::Error::other("unplugged")))
    }
}

/// Drives one fake pad. `send` returns once the device task has handled the
/// input and is waiting for the next one.
struct PadHandle {
    inputs: mpsc::UnboundedSender<io::Result<PadInput>>,
    polls: watch::Receiver<usize>,
    sent: usize,
}

impl PadHandle {
    async fn send(&mut self, input: PadInput) {
        self.inputs.send(Ok(input)).unwrap();
        self.sent += 1;
        let sent = self.sent;
        self.polls.wait_for(|polls| *polls > sent).await.unwrap();
    }

    fn fail(&self) {
        self.inputs
            .send(Err(io::Error::other("no such device")))
            .unwrap();
    }
}

#[derive(Default)]
struct FakePlatform {
    hotplug: Mutex<Option<mpsc::UnboundedReceiver<Result<Hotplug, BackendError>>>>,
    joysticks: Mutex<Option<Result<Vec<PathBuf>, BackendError>>>,
    opens: Mutex<HashMap<PathBuf, VecDeque<io::Result<FakePad>>>>,
    open_calls: Mutex<Vec<PathBuf>>,
}

impl Platform for FakePlatform {
    type Pad = FakePad;

    fn monitor(&self) -> impl Future<Output = Result<HotplugReceiver, BackendError>> + Send {
        future::ready(
            lock(&self.hotplug)
                .take()
                .map(HotplugReceiver::new)
                .ok_or_else(|| BackendError::Unavailable("no udev".into())),
        )
    }

    fn joysticks(&self) -> Result<Vec<PathBuf>, BackendError> {
        lock(&self.joysticks).clone().unwrap_or(Ok(Vec::new()))
    }

    fn open(&self, node: &Path) -> io::Result<FakePad> {
        lock(&self.open_calls).push(node.to_path_buf());
        lock(&self.opens)
            .get_mut(node)
            .and_then(VecDeque::pop_front)
            .unwrap_or_else(|| Err(io::ErrorKind::NotFound.into()))
    }
}

struct Harness {
    watcher: Arc<Watcher<FakePlatform>>,
    hotplug: mpsc::UnboundedSender<Result<Hotplug, BackendError>>,
    sink: Arc<RecordingSink>,
    clock: FakeClock,
    task: Option<JoinHandle<Result<(), BackendError>>>,
}

fn stick() -> Axes {
    Axes::new([(
        Abs::ABS_X.0,
        AxisRange {
            min: -32768,
            max: 32767,
            flat: 0,
            value: 0,
        },
    )])
}

fn axis(value: i32) -> PadInput {
    PadInput::Absolute {
        code: Abs::ABS_X.0,
        value,
    }
}

impl Harness {
    fn new(settings: &GamepadSettings) -> Self {
        let (hotplug, rx) = mpsc::unbounded_channel();
        let platform = FakePlatform::default();
        *lock(&platform.hotplug) = Some(rx);
        let clock = FakeClock::new();
        Self {
            watcher: Arc::new(Watcher::new(platform, settings, Arc::new(clock.clone()))),
            hotplug,
            sink: Arc::new(RecordingSink::new()),
            clock,
            task: None,
        }
    }

    fn platform(&self) -> &FakePlatform {
        &self.watcher.platform
    }

    /// Queues an openable pad for the next `open(node)`.
    fn plug(&self, node: &str, name: &str) -> PadHandle {
        self.plug_with(node, name, &[Key::BTN_SOUTH], stick())
    }

    /// Queues an openable device with these capabilities for the next
    /// `open(node)`.
    fn plug_with(&self, node: &str, name: &str, keys: &[Key], axes: Axes) -> PadHandle {
        let (inputs, rx) = mpsc::unbounded_channel();
        let (polls_tx, polls) = watch::channel(0);
        let pad = FakePad {
            name: name.into(),
            keys: keys.iter().map(|key| key.0).collect(),
            axes,
            inputs: rx,
            polls: polls_tx,
        };
        self.queue_open(node, Ok(pad));
        PadHandle {
            inputs,
            polls,
            sent: 0,
        }
    }

    fn queue_open(&self, node: &str, result: io::Result<FakePad>) {
        lock(&self.platform().opens)
            .entry(node.into())
            .or_default()
            .push_back(result);
    }

    fn present(&self, nodes: &[&str]) {
        let nodes = nodes.iter().map(PathBuf::from).collect();
        *lock(&self.platform().joysticks) = Some(Ok(nodes));
    }

    fn open_calls(&self) -> usize {
        lock(&self.platform().open_calls).len()
    }

    fn start(&mut self) {
        let watcher = Arc::clone(&self.watcher);
        let sink = self.sink.clone();
        self.task = Some(tokio::spawn(async move { watcher.watch(sink).await }));
    }

    fn change(&self, change: Hotplug) {
        self.hotplug.send(Ok(change)).unwrap();
    }

    fn device_ids(&self) -> Vec<String> {
        self.watcher
            .shared
            .snapshot()
            .into_iter()
            .map(|device| device.id)
            .collect()
    }

    fn activity(&self) -> Vec<String> {
        self.sink
            .take()
            .into_iter()
            .map(|event| match event {
                Event::Activity(ActivityEvent::GamepadActivity { device }) => device,
                other => panic!("unexpected event {other:?}"),
            })
            .collect()
    }

    async fn result(&mut self) -> Result<(), BackendError> {
        self.task.take().unwrap().await.unwrap()
    }
}

async fn eventually(mut condition: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !condition() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn present_pads_emit_past_the_deadzone_only() {
    let mut h = Harness::new(&GamepadSettings::default());
    h.present(&[PAD]);
    let mut pad = h.plug(PAD, "Xbox Wireless Controller");
    h.start();
    eventually(|| h.device_ids() == [PAD]).await;

    pad.send(axis(1500)).await;
    pad.send(PadInput::Other).await;
    assert_eq!(h.activity(), Vec::<String>::new());
    assert_eq!(h.watcher.shared.snapshot()[0].last_activity, None);

    pad.send(axis(20_000)).await;
    assert_eq!(h.activity(), [PAD]);
    assert_eq!(
        h.watcher.shared.snapshot()[0].last_activity,
        Some(h.clock.now())
    );
}

#[tokio::test]
async fn buttons_and_hats_count_and_bursts_are_rate_limited() {
    let mut h = Harness::new(&GamepadSettings::default());
    h.present(&[PAD]);
    let mut pad = h.plug(PAD, "8BitDo Pro 2");
    h.start();
    eventually(|| h.device_ids() == [PAD]).await;

    pad.send(PadInput::Button).await;
    pad.send(PadInput::Button).await;
    pad.send(axis(-30_000)).await;
    assert_eq!(h.activity(), [PAD]);

    h.clock
        .advance(ACTIVITY_INTERVAL.saturating_sub(Duration::from_millis(1)));
    pad.send(PadInput::Relative).await;
    assert_eq!(h.activity(), Vec::<String>::new());

    h.clock.advance(Duration::from_millis(1));
    pad.send(PadInput::Button).await;
    assert_eq!(h.activity(), [PAD]);
}

#[tokio::test]
async fn ignored_pads_never_emit_but_still_show_activity() {
    let settings = GamepadSettings {
        ignore_devices: vec!["drifty".into()],
        ..GamepadSettings::default()
    };
    let mut h = Harness::new(&settings);
    h.present(&[PAD]);
    let mut pad = h.plug(PAD, "Drifty Pad");
    h.start();
    eventually(|| h.device_ids() == [PAD]).await;

    pad.send(PadInput::Button).await;
    h.clock.advance(Duration::from_secs(5));
    pad.send(axis(32_767)).await;
    assert_eq!(h.activity(), Vec::<String>::new());
    let device = &h.watcher.shared.snapshot()[0];
    assert!(device.ignored);
    assert_eq!(device.name, "Drifty Pad");
    assert_eq!(device.last_activity, Some(h.clock.now()));
}

#[tokio::test]
async fn reload_changes_filtering_without_reopening() {
    let mut h = Harness::new(&GamepadSettings::default());
    h.present(&[PAD]);
    let mut pad = h.plug(PAD, "Fanatec Wheel");
    h.start();
    eventually(|| h.device_ids() == [PAD]).await;

    h.watcher.shared.apply(&GamepadSettings {
        deadzone_percent: 50,
        ignore_devices: Vec::new(),
    });
    pad.send(axis(10_000)).await;
    assert_eq!(h.activity(), Vec::<String>::new());
    pad.send(axis(20_000)).await;
    assert_eq!(h.activity(), [PAD]);

    h.watcher.shared.apply(&GamepadSettings {
        deadzone_percent: 50,
        ignore_devices: vec!["fanatec".into()],
    });
    h.clock.advance(ACTIVITY_INTERVAL);
    pad.send(PadInput::Button).await;
    assert_eq!(h.activity(), Vec::<String>::new());
    assert!(h.watcher.shared.snapshot()[0].ignored);
    assert_eq!(h.open_calls(), 1);
}

#[tokio::test]
async fn hotplug_opens_new_pads_and_drops_removed_ones() {
    let mut h = Harness::new(&GamepadSettings::default());
    h.start();
    let mut pad = h.plug(PAD, "DualSense");
    h.change(Hotplug::Added(PAD.into()));
    eventually(|| h.device_ids() == [PAD]).await;
    pad.send(PadInput::Button).await;
    assert_eq!(h.activity(), [PAD]);

    h.change(Hotplug::Added(PAD.into()));
    let mut other = h.plug(OTHER, "Xbox Controller");
    h.change(Hotplug::Added(OTHER.into()));
    eventually(|| h.device_ids() == [PAD, OTHER]).await;
    assert_eq!(h.open_calls(), 2);
    other.send(PadInput::Button).await;
    assert_eq!(h.activity(), [OTHER]);

    h.change(Hotplug::Removed(PAD.into()));
    eventually(|| h.device_ids() == [OTHER]).await;
    eventually(|| pad.inputs.is_closed()).await;

    h.change(Hotplug::Removed("/dev/input/event99".into()));
    let mut again = h.plug(PAD, "DualSense");
    h.change(Hotplug::Added(PAD.into()));
    eventually(|| h.device_ids() == [PAD, OTHER]).await;
    again.send(PadInput::Button).await;
    assert_eq!(h.activity(), [PAD]);
    assert!(!h.task.as_ref().unwrap().is_finished());
}

#[tokio::test]
async fn a_pad_whose_stream_fails_is_dropped() {
    let mut h = Harness::new(&GamepadSettings::default());
    h.present(&[PAD, OTHER]);
    let pad = h.plug(PAD, "Pad");
    let _other = h.plug(OTHER, "Other");
    h.start();
    eventually(|| h.device_ids() == [PAD, OTHER]).await;

    pad.fail();
    eventually(|| h.device_ids() == [OTHER]).await;
    h.change(Hotplug::Removed(PAD.into()));

    let _back = h.plug(PAD, "Pad");
    h.change(Hotplug::Added(PAD.into()));
    eventually(|| h.device_ids() == [PAD, OTHER]).await;
}

#[derive(Clone, Default)]
struct Logs(Arc<Mutex<Vec<u8>>>);

impl Write for Logs {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        lock(&self.0).extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Logs {
    /// Captures debug logs on this thread until the guard drops.
    fn capture() -> (Self, tracing::subscriber::DefaultGuard) {
        let logs = Self::default();
        let writer = logs.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(move || writer.clone())
            .with_ansi(false)
            .with_max_level(tracing::Level::DEBUG)
            .finish();
        (logs, tracing::subscriber::set_default(subscriber))
    }

    fn count(&self, needle: &str) -> usize {
        String::from_utf8_lossy(&lock(&self.0))
            .matches(needle)
            .count()
    }
}

fn hats() -> Axes {
    let hat = AxisRange {
        min: -1,
        max: 1,
        flat: 0,
        value: 0,
    };
    Axes::new([(Abs::ABS_HAT0X.0, hat), (Abs::ABS_HAT0Y.0, hat)])
}

#[tokio::test]
async fn nodes_that_arent_gamepads_are_closed_skipped_and_logged_once() {
    let (logs, _guard) = Logs::capture();
    let mut h = Harness::new(&GamepadSettings::default());
    h.present(&[KEYBOARD, PAD]);
    let keyboard = h.plug_with(
        KEYBOARD,
        "Keychron K5 System Control",
        &[Key::KEY_POWER, Key::KEY_SLEEP],
        hats(),
    );
    let mut pad = h.plug(PAD, "Xbox Controller");
    h.start();
    eventually(|| h.device_ids() == [PAD]).await;
    eventually(|| keyboard.inputs.is_closed()).await;

    h.change(Hotplug::Added(KEYBOARD.into()));
    let stream = h.plug_with(
        OTHER,
        "stream-controller",
        &[Key::KEY_A, Key::BTN_TRIGGER],
        Axes::default(),
    );
    h.change(Hotplug::Added(OTHER.into()));
    eventually(|| h.open_calls() == 3).await;
    eventually(|| stream.inputs.is_closed()).await;
    assert_eq!(h.device_ids(), [PAD]);
    assert_eq!(logs.count("not a gamepad"), 2);
    assert_eq!(logs.count("Keychron K5 System Control"), 1);
    assert_eq!(logs.count("no joystick or gamepad buttons"), 1);
    assert_eq!(logs.count("no absolute axes"), 1);

    pad.send(PadInput::Button).await;
    assert_eq!(h.activity(), [PAD]);

    h.change(Hotplug::Removed(KEYBOARD.into()));
    let _replugged = h.plug(KEYBOARD, "Flight Stick");
    h.change(Hotplug::Added(KEYBOARD.into()));
    eventually(|| h.device_ids() == [KEYBOARD, PAD]).await;
    assert_eq!(h.open_calls(), 4);
}

#[tokio::test]
async fn a_button_box_with_no_axes_counts_as_a_gamepad() {
    let mut h = Harness::new(&GamepadSettings::default());
    h.present(&[PAD]);
    let mut pad = h.plug_with(PAD, "Button Box", &[Key::BTN_TRIGGER], Axes::default());
    h.start();
    eventually(|| h.device_ids() == [PAD]).await;
    pad.send(PadInput::Button).await;
    assert_eq!(h.activity(), [PAD]);
}

#[tokio::test]
async fn permission_errors_are_logged_once_and_dont_stop_the_source() {
    let (logs, _guard) = Logs::capture();

    let mut h = Harness::new(&GamepadSettings::default());
    h.present(&[PAD]);
    for _ in 0..4 {
        h.queue_open(PAD, Err(io::ErrorKind::PermissionDenied.into()));
    }
    h.start();
    h.change(Hotplug::Added(PAD.into()));
    h.change(Hotplug::Added(PAD.into()));
    h.change(Hotplug::Added(OTHER.into()));
    eventually(|| h.open_calls() == 4).await;
    assert_eq!(h.device_ids(), Vec::<String>::new());
    assert_eq!(logs.count("can't open gamepad; it needs"), 1);
    assert_eq!(logs.count("can't open gamepad"), 2);

    h.change(Hotplug::Removed(PAD.into()));
    h.change(Hotplug::Added(PAD.into()));
    eventually(|| h.open_calls() == 5).await;
    assert_eq!(logs.count("can't open gamepad; it needs"), 2);

    let mut pad = h.plug(PAD, "Pad");
    h.change(Hotplug::Added(PAD.into()));
    eventually(|| h.device_ids() == [PAD]).await;
    pad.send(PadInput::Button).await;
    assert_eq!(h.activity(), [PAD]);
    assert!(!h.task.as_ref().unwrap().is_finished());
}

#[tokio::test]
async fn monitor_failures_end_the_watch() {
    let mut h = Harness::new(&GamepadSettings::default());
    h.present(&[PAD]);
    let _pad = h.plug(PAD, "Pad");
    h.start();
    eventually(|| h.device_ids() == [PAD]).await;

    h.hotplug
        .send(Err(BackendError::Io("netlink overrun".into())))
        .unwrap();
    assert_eq!(
        h.result().await,
        Err(BackendError::Io("netlink overrun".into()))
    );
    assert_eq!(h.device_ids(), Vec::<String>::new());
}

#[tokio::test]
async fn startup_failures_are_reported() {
    let mut h = Harness::new(&GamepadSettings::default());
    *lock(&h.platform().joysticks) = Some(Err(BackendError::Unavailable("no sysfs".into())));
    h.start();
    assert_eq!(
        h.result().await,
        Err(BackendError::Unavailable("no sysfs".into()))
    );

    h.start();
    assert_eq!(
        h.result().await,
        Err(BackendError::Unavailable("no udev".into()))
    );
}

#[tokio::test]
async fn dropping_the_watch_closes_every_pad() {
    let mut h = Harness::new(&GamepadSettings::default());
    h.present(&[PAD]);
    let pad = h.plug(PAD, "Pad");
    h.start();
    eventually(|| h.device_ids() == [PAD]).await;

    let task = h.task.take().unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert_eq!(h.device_ids(), Vec::<String>::new());
    eventually(|| pad.inputs.is_closed()).await;
}

#[test]
fn construction_opens_nothing() {
    let source = EvdevGamepadSource::new(&GamepadSettings::default());
    assert_eq!(source.devices(), []);
    source.update_settings(&GamepadSettings {
        deadzone_percent: 30,
        ignore_devices: vec!["pedals".into()],
    });
    assert_eq!(source.devices(), []);
}
