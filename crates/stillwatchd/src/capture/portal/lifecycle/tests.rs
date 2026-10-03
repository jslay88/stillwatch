use std::time::Duration;

use stillwatch_core::backend::BackendError;
use stillwatch_core::state::State;
use stillwatch_core::time::FakeClock;

use super::{CaptureWant, CeilingCapture, Order, Presence, SessionMachine, presence_for};

fn away(machine: &mut SessionMachine, clock: &FakeClock) -> Order {
    machine.set_presence(Presence::Away, clock)
}

fn active(machine: &mut SessionMachine, clock: &FakeClock) -> Order {
    machine.set_presence(Presence::Active, clock)
}

#[test]
fn starts_when_idle_and_stops_when_active() {
    let clock = FakeClock::new();
    let mut machine = SessionMachine::new();
    assert_eq!(away(&mut machine, &clock), Order::Start);
    assert!(!machine.streaming());
    machine.started(&clock);
    assert!(machine.streaming());
    assert_eq!(away(&mut machine, &clock), Order::None);

    assert_eq!(active(&mut machine, &clock), Order::Stop);
    assert!(!machine.streaming());
    clock.advance(Duration::from_secs(120));
    assert_eq!(machine.poll(&clock), Order::None);
    assert_eq!(away(&mut machine, &clock), Order::Start);
}

#[test]
fn active_never_starts_a_stream() {
    let clock = FakeClock::new();
    let mut machine = SessionMachine::new();
    assert_eq!(active(&mut machine, &clock), Order::None);
    clock.advance(Duration::from_secs(120));
    assert_eq!(machine.poll(&clock), Order::None);
    assert!(!machine.streaming());
}

#[test]
fn denial_blocks_until_restart_and_disconnects_back_off() {
    let clock = FakeClock::new();
    let mut machine = SessionMachine::new();
    assert_eq!(away(&mut machine, &clock), Order::Start);
    let denied = BackendError::PermissionDenied("cancelled".into());
    assert_eq!(machine.failed(&denied, &clock), Order::Stop);
    assert!(machine.blocked());
    clock.advance(Duration::from_secs(120));
    assert_eq!(machine.poll(&clock), Order::None);
    assert_eq!(away(&mut machine, &clock), Order::None);

    let mut machine = SessionMachine::new();
    assert_eq!(away(&mut machine, &clock), Order::Start);
    machine.started(&clock);
    clock.advance(Duration::from_secs(2));
    let dropped = BackendError::Disconnected("session closed".into());
    assert_eq!(machine.failed(&dropped, &clock), Order::Stop);
    assert!(!machine.blocked());
    assert_eq!(machine.poll(&clock), Order::None);
    clock.advance(Duration::from_secs(1));
    assert_eq!(machine.poll(&clock), Order::Start);
}

#[test]
fn activity_during_back_off_cancels_the_wait() {
    let clock = FakeClock::new();
    let mut machine = SessionMachine::new();
    away(&mut machine, &clock);
    machine.failed(&BackendError::Io("pipewire".into()), &clock);
    assert_eq!(active(&mut machine, &clock), Order::None);
    assert_eq!(away(&mut machine, &clock), Order::Start);
}

#[test]
fn presence_follows_monitoring_and_the_ceiling() {
    let base = CaptureWant {
        state: State::Monitoring,
        idle: true,
        asleep: false,
        ceiling: CeilingCapture {
            enabled: true,
            during_pause: false,
        },
    };
    assert_eq!(presence_for(base), Presence::Away);
    assert_eq!(
        presence_for(CaptureWant {
            idle: false,
            ..base
        }),
        Presence::Active
    );
    assert_eq!(
        presence_for(CaptureWant {
            asleep: true,
            ..base
        }),
        Presence::Active
    );
    assert_eq!(
        presence_for(CaptureWant {
            state: State::Snoozed,
            ..base
        }),
        Presence::Away
    );
    assert_eq!(
        presence_for(CaptureWant {
            state: State::Snoozed,
            ceiling: CeilingCapture {
                enabled: false,
                during_pause: false,
            },
            ..base
        }),
        Presence::Active
    );
    assert_eq!(
        presence_for(CaptureWant {
            state: State::Paused,
            ..base
        }),
        Presence::Active
    );
    assert_eq!(
        presence_for(CaptureWant {
            state: State::Paused,
            ceiling: CeilingCapture {
                enabled: true,
                during_pause: true,
            },
            ..base
        }),
        Presence::Away
    );
    for state in [
        State::Active,
        State::Prompting,
        State::Acting,
        State::Blanked,
        State::Locked,
    ] {
        assert_eq!(
            presence_for(CaptureWant { state, ..base }),
            Presence::Active,
            "{state:?}"
        );
    }
}
