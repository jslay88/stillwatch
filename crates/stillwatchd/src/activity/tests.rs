use std::collections::VecDeque;
use std::sync::Mutex;

use stillwatch_core::activity::{Presence, WakeSource};
use stillwatch_core::backend::GamepadDevice;
use stillwatch_core::backoff::BackoffPolicy;
use stillwatch_core::event::ActivityEvent;
use stillwatch_core::mocks::{MockGamepadSource, MockIdleSource, WatchEnd};

use super::*;
use crate::clock::TokioClock;

const PAD: &str = "/dev/input/event7";

type Seen = Vec<(Duration, ActivityOutput)>;

fn mins(n: u64) -> Duration {
    Duration::from_mins(n)
}

fn backoff() -> Backoff {
    Backoff::new(BackoffPolicy {
        initial: Duration::from_secs(1),
        ..BackoffPolicy::default()
    })
}

fn settings(input_idle: Duration) -> ActivitySettings {
    ActivitySettings {
        input_idle,
        gamepad: true,
    }
}

fn pad_event() -> Event {
    ActivityEvent::GamepadActivity { device: PAD.into() }.into()
}

fn machine(event: ActivityEvent) -> ActivityOutput {
    ActivityOutput::Machine(event)
}

fn changed(presence: Presence) -> ActivityOutput {
    ActivityOutput::Changed(presence)
}

/// Plays each `watch` call's events at set offsets from the call, then hangs.
#[derive(Default)]
struct TimedSource {
    runs: Mutex<VecDeque<Vec<(Duration, Event)>>>,
    timeouts: Mutex<Vec<Duration>>,
}

impl TimedSource {
    fn with_runs(runs: Vec<Vec<(Duration, Event)>>) -> Self {
        Self {
            runs: Mutex::new(runs.into()),
            ..Self::default()
        }
    }

    fn play(&self, sink: Arc<dyn EventSink>) -> BackendFuture<'static, ()> {
        let run = self.runs.lock().unwrap().pop_front().unwrap_or_default();
        Box::pin(async move {
            let start = tokio::time::Instant::now();
            for (at, event) in run {
                tokio::time::sleep_until(start + at).await;
                sink.send(event);
            }
            std::future::pending().await
        })
    }
}

impl IdleSource for TimedSource {
    fn watch(&self, timeout: Duration, sink: Arc<dyn EventSink>) -> BackendFuture<'_, ()> {
        self.timeouts.lock().unwrap().push(timeout);
        self.play(sink)
    }
}

impl GamepadSource for TimedSource {
    fn watch(&self, sink: Arc<dyn EventSink>) -> BackendFuture<'_, ()> {
        self.play(sink)
    }

    fn devices(&self) -> Vec<GamepadDevice> {
        Vec::new()
    }
}

/// Runs the loop for `span` of paused time and returns what it emitted, each
/// with its offset from the start. The loop must still be running.
async fn collect(
    sources: ActivitySources<'_>,
    settings: watch::Receiver<ActivitySettings>,
    span: Duration,
) -> Seen {
    let start = TokioClock.now();
    let mut seen = Vec::new();
    let running = run(sources, settings, backoff(), &TokioClock, |output| {
        seen.push((TokioClock.now() - start, output));
    });
    assert!(tokio::time::timeout(span, running).await.is_err());
    seen
}

#[tokio::test(start_paused = true)]
async fn compositor_idle_with_quiet_pads_goes_idle() {
    let idle = MockIdleSource::new();
    idle.push_run(vec![ActivityEvent::InputIdle.into()], WatchEnd::Hang);
    let pads = MockGamepadSource::new();
    let (_tx, rx) = watch::channel(settings(mins(10)));
    let sources = ActivitySources {
        idle: &idle,
        gamepad: Some(&pads),
    };
    let seen = collect(sources, rx, mins(30)).await;
    assert_eq!(
        seen,
        [
            (Duration::ZERO, machine(ActivityEvent::InputIdle)),
            (Duration::ZERO, changed(Presence::Idle)),
        ]
    );
    assert_eq!(idle.timeouts(), [mins(10)]);
    assert_eq!(pads.watch_calls(), 1);
}

#[tokio::test(start_paused = true)]
async fn a_pad_shortly_before_compositor_idle_delays_idle_until_the_timeout_after_it() {
    let idle = TimedSource::with_runs(vec![vec![(mins(9), ActivityEvent::InputIdle.into())]]);
    let pads = TimedSource::with_runs(vec![vec![(mins(1), pad_event())]]);
    let (_tx, rx) = watch::channel(settings(mins(10)));
    let sources = ActivitySources {
        idle: &idle,
        gamepad: Some(&pads),
    };
    let seen = collect(sources, rx, mins(30)).await;
    assert_eq!(
        seen,
        [
            (
                mins(1),
                ActivityOutput::Machine(ActivityEvent::GamepadActivity { device: PAD.into() })
            ),
            (mins(11), machine(ActivityEvent::InputIdle)),
            (mins(11), changed(Presence::Idle)),
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn a_reconnected_watch_counts_as_active_until_the_compositor_says_otherwise() {
    let idle = MockIdleSource::new();
    let lost = BackendError::Disconnected("compositor restarted".into());
    idle.push_run(vec![ActivityEvent::InputIdle.into()], WatchEnd::Fail(lost));
    idle.push_run(vec![ActivityEvent::InputIdle.into()], WatchEnd::Hang);
    let (_tx, rx) = watch::channel(settings(mins(10)));
    let sources = ActivitySources {
        idle: &idle,
        gamepad: None,
    };
    let seen = collect(sources, rx, mins(1)).await;
    let reconnect = Duration::from_secs(1);
    assert_eq!(
        seen,
        [
            (Duration::ZERO, machine(ActivityEvent::InputIdle)),
            (Duration::ZERO, changed(Presence::Idle)),
            (reconnect, machine(ActivityEvent::InputResumed)),
            (
                reconnect,
                changed(Presence::Active(WakeSource::WatchRestarted))
            ),
            (reconnect, machine(ActivityEvent::InputIdle)),
            (reconnect, changed(Presence::Idle)),
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn a_new_timeout_recreates_the_watch_and_applies_to_pads() {
    let idle = TimedSource::with_runs(vec![
        vec![(mins(1), ActivityEvent::InputIdle.into())],
        vec![(mins(3), ActivityEvent::InputIdle.into())],
    ]);
    let pads = TimedSource::with_runs(vec![vec![(Duration::ZERO, pad_event())]]);
    let (tx, rx) = watch::channel(settings(mins(10)));
    let sources = ActivitySources {
        idle: &idle,
        gamepad: Some(&pads),
    };
    let reload = async {
        tokio::time::sleep(mins(2)).await;
        tx.send_replace(settings(mins(3)));
    };
    let (seen, ()) = tokio::join!(collect(sources, rx, mins(30)), reload);
    assert_eq!(*idle.timeouts.lock().unwrap(), [mins(10), mins(3)]);
    assert_eq!(
        seen[1..],
        [
            (mins(5), machine(ActivityEvent::InputIdle)),
            (mins(5), changed(Presence::Idle)),
        ],
        "the restart forgets compositor idle, and the new watch reports it 3m later"
    );
}

#[tokio::test(start_paused = true)]
async fn a_failed_gamepad_source_leaves_keyboard_and_mouse_idle_running() {
    let idle = TimedSource::with_runs(vec![vec![(mins(10), ActivityEvent::InputIdle.into())]]);
    let pads = MockGamepadSource::new();
    let gone = BackendError::Unavailable("udev".into());
    pads.push_run(vec![pad_event()], WatchEnd::Fail(gone));
    let (tx, rx) = watch::channel(settings(mins(10)));
    drop(tx);
    let sources = ActivitySources {
        idle: &idle,
        gamepad: Some(&pads),
    };
    let seen = collect(sources, rx, mins(30)).await;
    assert_eq!(seen.last(), Some(&(mins(10), changed(Presence::Idle))));
    assert_eq!(pads.watch_calls(), 1);
}

#[tokio::test(start_paused = true)]
async fn an_unsupported_compositor_ends_the_loop() {
    let idle = MockIdleSource::new();
    let v1 = BackendError::Unsupported(idle::V1_ONLY.into());
    idle.push_run(Vec::new(), WatchEnd::Fail(v1.clone()));
    let (_tx, rx) = watch::channel(settings(mins(10)));
    let sources = ActivitySources {
        idle: &idle,
        gamepad: None,
    };
    let result = run(sources, rx, backoff(), &TokioClock, |_| {}).await;
    assert_eq!(result, Err(v1));
}
