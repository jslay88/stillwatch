use std::sync::Arc;
use std::time::Duration;

use super::*;
use crate::backend::{BackendError, GamepadSource, IdleSource};
use crate::event::{ControlCommand, SessionEvent};
use crate::mocks::{
    Harness, MockGamepadSource, MockIdleSource, RecordingSink, WatchEnd, now_or_never,
};
use crate::state::State;
use crate::time::{Clock, FakeClock, TimerQueue};

const PAD: &str = "/dev/input/event7";

fn mins(n: u64) -> Duration {
    Duration::from_mins(n)
}

fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

fn pad() -> RawActivity {
    RawActivity::Gamepad { device: PAD.into() }
}

fn idle_out() -> Vec<ActivityOutput> {
    vec![
        ActivityOutput::Machine(ActivityEvent::InputIdle),
        ActivityOutput::Changed(Presence::Idle),
    ]
}

fn set_timer(after: Duration) -> ActivityOutput {
    ActivityOutput::Timer(Command::SetTimer {
        id: TimerId::GamepadIdle,
        after,
    })
}

fn cancel_timer() -> ActivityOutput {
    ActivityOutput::Timer(Command::CancelTimer(TimerId::GamepadIdle))
}

fn pad_event() -> ActivityOutput {
    ActivityOutput::Machine(ActivityEvent::GamepadActivity { device: PAD.into() })
}

fn woke(source: WakeSource) -> ActivityOutput {
    ActivityOutput::Changed(Presence::Active(source))
}

/// Drives an aggregator the way the daemon does: a fake clock, and a timer
/// queue whose firings come back as `GamepadIdle`.
struct Rig {
    clock: FakeClock,
    aggregator: ActivityAggregator,
    timers: TimerQueue,
}

impl Rig {
    fn new() -> Self {
        Self::with(ActivitySettings::default())
    }

    fn with(settings: ActivitySettings) -> Self {
        Self {
            clock: FakeClock::new(),
            aggregator: ActivityAggregator::new(settings),
            timers: TimerQueue::new(),
        }
    }

    fn absorb(&mut self, outputs: Vec<ActivityOutput>) -> Vec<ActivityOutput> {
        for output in &outputs {
            if let ActivityOutput::Timer(command) = output {
                assert!(self.timers.apply(self.clock.now(), command));
            }
        }
        outputs
    }

    fn send(&mut self, raw: RawActivity) -> Vec<ActivityOutput> {
        let outputs = self.aggregator.handle(self.clock.now(), raw);
        self.absorb(outputs)
    }

    fn reload(&mut self, settings: ActivitySettings) -> Vec<ActivityOutput> {
        let outputs = self.aggregator.apply_settings(self.clock.now(), settings);
        self.absorb(outputs)
    }

    /// Advances `by`, feeding every timer that comes due back in.
    fn advance(&mut self, by: Duration) -> Vec<ActivityOutput> {
        let target = self.clock.now() + by;
        let mut outputs = Vec::new();
        while let Some(at) = self.timers.next_deadline().filter(|at| *at <= target) {
            self.clock.advance(at - self.clock.now());
            for id in self.timers.pop_due(at) {
                let raw = RawActivity::from_event(&Event::Timer(id)).unwrap();
                outputs.extend(self.send(raw));
            }
        }
        self.clock.advance(target - self.clock.now());
        outputs
    }
}

/// Plays a mock source's scripted run and returns what reached the sink.
fn played(
    watch: impl FnOnce(Arc<RecordingSink>) -> Option<Result<(), BackendError>>,
) -> Vec<Event> {
    let sink = Arc::new(RecordingSink::new());
    assert_eq!(watch(Arc::clone(&sink)), None, "scripted runs hang");
    sink.take()
}

fn raw(events: &[Event]) -> Vec<RawActivity> {
    events.iter().filter_map(RawActivity::from_event).collect()
}

#[test]
fn compositor_idle_with_no_pad_is_idle_at_once() {
    let idle = MockIdleSource::new();
    idle.push_run(vec![ActivityEvent::InputIdle.into()], WatchEnd::Hang);
    let events = played(|sink| now_or_never(idle.watch(mins(10), sink)));

    let mut rig = Rig::new();
    let outputs: Vec<_> = raw(&events).into_iter().flat_map(|r| rig.send(r)).collect();
    assert_eq!(outputs, idle_out());
    assert!(rig.aggregator.is_idle());
    assert!(rig.timers.is_empty());
}

#[test]
fn recent_pad_delays_idle_until_the_timeout_after_it() {
    let mut rig = Rig::new();
    assert_eq!(rig.send(pad()), vec![pad_event()]);
    rig.advance(mins(9));

    assert_eq!(
        rig.send(RawActivity::CompositorIdle),
        vec![set_timer(mins(1))]
    );
    assert!(!rig.aggregator.is_idle());
    assert_eq!(rig.advance(secs(59)), vec![]);
    assert_eq!(rig.advance(secs(1)), idle_out());
    assert!(rig.timers.is_empty());
}

#[test]
fn pad_during_idle_wakes_then_idles_again_once_quiet() {
    let mut rig = Rig::new();
    rig.send(RawActivity::CompositorIdle);

    assert_eq!(
        rig.send(pad()),
        vec![
            pad_event(),
            woke(WakeSource::Gamepad { device: PAD.into() }),
            set_timer(mins(10))
        ]
    );
    rig.advance(mins(4));
    assert_eq!(rig.send(pad()), vec![pad_event(), set_timer(mins(10))]);
    assert_eq!(rig.advance(mins(9)), vec![]);
    assert_eq!(rig.advance(mins(1)), idle_out());
}

#[test]
fn pads_that_stay_inside_the_deadzone_send_nothing_so_idle_follows_the_compositor() {
    let pads = MockGamepadSource::new();
    pads.push_run(Vec::new(), WatchEnd::Hang);
    let events = played(|sink| now_or_never(pads.watch(sink)));
    assert_eq!(events, vec![]);

    let mut rig = Rig::new();
    assert_eq!(rig.send(RawActivity::CompositorIdle), idle_out());
}

#[test]
fn keyboard_resume_wakes_and_drops_the_pad_timer() {
    let mut rig = Rig::new();
    rig.send(RawActivity::CompositorIdle);
    assert_eq!(
        rig.send(RawActivity::CompositorResumed),
        vec![
            ActivityOutput::Machine(ActivityEvent::InputResumed),
            woke(WakeSource::KeyboardMouse)
        ]
    );

    rig.send(pad());
    rig.send(RawActivity::CompositorIdle);
    assert!(rig.timers.contains(TimerId::GamepadIdle));
    assert_eq!(
        rig.send(RawActivity::CompositorResumed),
        vec![
            ActivityOutput::Machine(ActivityEvent::InputResumed),
            cancel_timer()
        ],
        "already active: forwarded so it can wake displays, but no change"
    );
    assert!(rig.timers.is_empty());
    assert_eq!(rig.advance(mins(30)), vec![]);
}

#[test]
fn pad_while_the_compositor_is_active_arms_nothing() {
    let mut rig = Rig::new();
    assert_eq!(rig.send(pad()), vec![pad_event()]);
    assert!(rig.timers.is_empty());
    rig.advance(mins(10));
    assert_eq!(rig.send(RawActivity::CompositorIdle), idle_out());
}

#[test]
fn an_early_timer_rearms_for_the_rest() {
    let mut rig = Rig::new();
    rig.send(pad());
    rig.send(RawActivity::CompositorIdle);
    rig.clock.advance(mins(6));
    assert_eq!(rig.send(RawActivity::GamepadIdle), vec![set_timer(mins(4))]);
}

#[test]
fn a_restarted_watch_counts_as_active() {
    let mut rig = Rig::new();
    assert_eq!(rig.send(RawActivity::WatchRestarted), vec![]);

    rig.send(RawActivity::CompositorIdle);
    assert_eq!(
        rig.send(RawActivity::WatchRestarted),
        vec![
            ActivityOutput::Machine(ActivityEvent::InputResumed),
            woke(WakeSource::WatchRestarted)
        ]
    );
    assert!(!rig.aggregator.is_idle());
    assert_eq!(rig.send(pad()), vec![pad_event()], "compositor unknown");

    rig.advance(mins(10));
    assert_eq!(rig.send(RawActivity::CompositorIdle), idle_out());
}

#[test]
fn a_restart_while_waiting_on_a_pad_cancels_the_timer() {
    let mut rig = Rig::new();
    rig.send(pad());
    rig.send(RawActivity::CompositorIdle);
    assert_eq!(rig.send(RawActivity::WatchRestarted), vec![cancel_timer()]);
    assert_eq!(rig.advance(mins(30)), vec![]);
}

#[test]
fn a_shorter_timeout_on_reload_can_idle_right_away() {
    let mut rig = Rig::new();
    rig.send(pad());
    rig.clock.advance(mins(3));
    rig.send(RawActivity::CompositorIdle);
    assert_eq!(
        rig.timers.deadline(TimerId::GamepadIdle),
        Some(rig.clock.now() + mins(7))
    );

    let five = ActivitySettings {
        input_idle: mins(5),
        ..ActivitySettings::default()
    };
    assert_eq!(rig.reload(five.clone()), vec![set_timer(mins(2))]);
    assert_eq!(rig.aggregator.settings(), &five);

    let two = ActivitySettings {
        input_idle: mins(2),
        ..five
    };
    assert_eq!(rig.reload(two), [vec![cancel_timer()], idle_out()].concat());
}

#[test]
fn a_longer_timeout_on_reload_waits_longer_but_idle_stays_idle() {
    let mut rig = Rig::new();
    rig.send(pad());
    rig.send(RawActivity::CompositorIdle);
    let twenty = ActivitySettings {
        input_idle: mins(20),
        ..ActivitySettings::default()
    };
    assert_eq!(rig.reload(twenty.clone()), vec![set_timer(mins(20))]);
    assert_eq!(rig.advance(mins(19)), vec![]);
    assert_eq!(rig.advance(mins(1)), idle_out());

    assert_eq!(
        rig.reload(ActivitySettings {
            input_idle: mins(60),
            ..twenty
        }),
        vec![]
    );
    assert!(rig.aggregator.is_idle());
}

#[test]
fn gamepads_off_means_only_the_compositor_counts() {
    let off = ActivitySettings {
        gamepad: false,
        ..ActivitySettings::default()
    };
    let mut rig = Rig::with(off.clone());
    rig.send(RawActivity::CompositorIdle);
    assert_eq!(
        rig.send(pad()),
        vec![pad_event()],
        "forwarded for the machine to drop, but changes nothing"
    );
    assert!(rig.aggregator.is_idle());

    let mut rig = Rig::new();
    rig.send(pad());
    rig.send(RawActivity::CompositorIdle);
    assert_eq!(rig.reload(off), [vec![cancel_timer()], idle_out()].concat());
}

#[test]
fn settings_come_from_the_config() {
    let mut config = Config::default();
    config.idle.input_idle_minutes = 3;
    config.activity.gamepad = false;
    assert_eq!(
        ActivitySettings::from(&config),
        ActivitySettings {
            input_idle: mins(3),
            gamepad: false
        }
    );
    assert_eq!(
        ActivitySettings::default(),
        ActivitySettings {
            input_idle: mins(10),
            gamepad: true
        }
    );
}

#[test]
fn only_source_events_and_the_pad_timer_are_raw() {
    let cases = [
        (
            ActivityEvent::InputIdle.into(),
            Some(RawActivity::CompositorIdle),
        ),
        (
            ActivityEvent::InputResumed.into(),
            Some(RawActivity::CompositorResumed),
        ),
        (
            ActivityEvent::GamepadActivity { device: PAD.into() }.into(),
            Some(pad()),
        ),
        (
            Event::Timer(TimerId::GamepadIdle),
            Some(RawActivity::GamepadIdle),
        ),
        (Event::Timer(TimerId::Capture), None),
        (SessionEvent::Locked.into(), None),
        (ControlCommand::Pause.into(), None),
    ];
    for (event, expected) in cases {
        assert_eq!(RawActivity::from_event(&event), expected, "{event:?}");
    }
}

/// The machine only reaches Monitoring once the pad is quiet, and a pad
/// sends it back to Active, with the aggregator re-idling it afterwards.
#[test]
fn drives_the_state_machine_through_a_gamepad_session() {
    let mut rig = Rig::new();
    let mut machine = Harness::new();
    let feed = |outputs: Vec<ActivityOutput>, machine: &mut Harness| {
        for output in outputs {
            if let ActivityOutput::Machine(event) = output {
                machine.send(event);
            }
        }
    };

    feed(rig.send(pad()), &mut machine);
    machine.advance(mins(5));
    rig.advance(mins(5));
    feed(rig.send(RawActivity::CompositorIdle), &mut machine);
    assert_eq!(machine.state(), State::Active);

    let outputs = rig.advance(mins(5));
    machine.advance(mins(5));
    feed(outputs, &mut machine);
    assert_eq!(machine.state(), State::Monitoring);

    feed(rig.send(pad()), &mut machine);
    assert_eq!(machine.state(), State::Active);
    let outputs = rig.advance(mins(10));
    feed(outputs, &mut machine);
    assert_eq!(machine.state(), State::Monitoring);
}
