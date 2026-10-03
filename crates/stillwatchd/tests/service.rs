//! The control service over a peer-to-peer connection, driven through the
//! `stillwatch-ipc` proxy against a fake daemon handle.

use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt as _;
use jiff::Timestamp;
use stillwatch_core::backend::{BackendError, GamepadDevice, MediaPlayer};
use stillwatch_core::event::ControlCommand;
use stillwatch_core::history::{HistoryEntry, HistoryKind};
use stillwatch_core::prompt::PromptOutcome;
use stillwatch_core::state::{State, StatusSnapshot};
use stillwatch_ipc::gamepad::GamepadInfo;
use stillwatch_ipc::json::{from_json, from_json_lines};
use stillwatch_ipc::player::PlayerInfo;
use stillwatch_ipc::probe::ProbeSample;
use stillwatch_ipc::proxy::StillwatchProxy;
use stillwatch_ipc::status::{PanelCareStatus, StatusPayload};
use stillwatchd::service::fake::FakeHandle;
use stillwatchd::service::{DaemonStatus, ReloadReport, Service, ServiceError};
use tokio::time::timeout;
use zbus::connection::Builder;
use zbus::fdo;
use zbus::{Connection, Guid};

const WAIT: Duration = Duration::from_secs(5);

struct Peer {
    fake: Arc<FakeHandle>,
    service: Service,
    proxy: StillwatchProxy<'static>,
    _client: Connection,
}

async fn peer() -> Result<Peer, ServiceError> {
    let (server, client) = tokio::net::UnixStream::pair().map_err(zbus::Error::from)?;
    let server = Builder::unix_stream(server)
        .server(Guid::generate())?
        .p2p()
        .build();
    let client = Builder::unix_stream(client).p2p().build();
    let (server, client) = tokio::try_join!(server, client)?;
    let fake = Arc::new(FakeHandle::new());
    let service = Service::serve(server, Arc::clone(&fake) as _).await?;
    let proxy = StillwatchProxy::new(&client).await?;
    Ok(Peer {
        fake,
        service,
        proxy,
        _client: client,
    })
}

/// The message of an `InvalidArgs` reply, or the reply it was instead.
fn invalid_args(err: zbus::Error) -> Result<String, fdo::Error> {
    match fdo::Error::from(err) {
        fdo::Error::InvalidArgs(message) => Ok(message),
        other => Err(other),
    }
}

/// The message of a `Failed` reply, or the reply it was instead.
fn failed(err: zbus::Error) -> Result<String, fdo::Error> {
    match fdo::Error::from(err) {
        fdo::Error::Failed(message) => Ok(message),
        other => Err(other),
    }
}

fn mins(m: u64) -> Duration {
    Duration::from_mins(m)
}

#[tokio::test]
async fn status_carries_the_snapshot_and_daemon_fields() {
    let peer = peer().await.unwrap();
    let status = DaemonStatus {
        capture_backend: Some("kwin".into()),
        config_errors: vec!["stale.stale_percent: must be 1-100".into()],
        panel_care: Some(PanelCareStatus::default()),
        ..DaemonStatus::new(StatusSnapshot {
            state: State::Snoozed,
            in_state: Duration::from_secs(90),
            snooze_remaining: Some(mins(10)),
            idle: true,
            locked: false,
            media_playing: true,
            last_detection: None,
        })
    };
    peer.fake.update(|state| state.status = status.clone());

    let json = peer.proxy.status().await.unwrap();
    let payload: StatusPayload = from_json(&json).unwrap();
    assert_eq!(payload, status.into_payload());
    assert_eq!(payload.snooze_remaining_seconds, Some(600));
}

#[tokio::test]
async fn control_methods_become_control_commands() {
    let peer = peer().await.unwrap();
    peer.proxy.snooze(15 * 60).await.unwrap();
    peer.proxy.snooze(45 * 60).await.unwrap();
    peer.proxy.cancel_snooze().await.unwrap();
    peer.proxy.pause().await.unwrap();
    peer.proxy.resume().await.unwrap();
    assert_eq!(
        peer.fake.state().controls,
        [
            ControlCommand::Snooze(mins(15)),
            ControlCommand::Snooze(mins(45)),
            ControlCommand::CancelSnooze,
            ControlCommand::Pause,
            ControlCommand::Resume,
        ]
    );
}

#[tokio::test]
async fn snoozes_outside_the_rules_are_refused() {
    let peer = peer().await.unwrap();
    let short = peer.proxy.snooze(30).await.unwrap_err();
    assert_eq!(
        invalid_args(short).unwrap(),
        "snooze must be at least 1 minutes"
    );
    let long = peer.proxy.snooze(721 * 60).await.unwrap_err();
    assert_eq!(
        invalid_args(long).unwrap(),
        "snooze must be at most 720 minutes"
    );

    peer.fake.update(|state| state.prompt.allow_custom = false);
    let custom = peer.proxy.snooze(45 * 60).await.unwrap_err();
    let message = invalid_args(custom).unwrap();
    assert!(
        message.starts_with("custom snooze durations are disabled"),
        "{message}"
    );
    peer.proxy.snooze(60 * 60).await.unwrap();

    assert_eq!(
        peer.fake.state().controls,
        [ControlCommand::Snooze(mins(60))]
    );
}

#[tokio::test]
async fn reload_returns_the_report() {
    let peer = peer().await.unwrap();
    assert_eq!(peer.proxy.reload().await.unwrap(), (true, Vec::new()));
    let errors = vec!["stale.require: unknown variant".to_owned()];
    peer.fake
        .update(|state| state.reload = ReloadReport::rejected(errors.clone()));
    assert_eq!(peer.proxy.reload().await.unwrap(), (false, errors));
    assert_eq!(peer.fake.state().reloads, 2);
}

#[tokio::test]
async fn history_is_filtered_by_age() {
    let peer = peer().await.unwrap();
    let old = HistoryEntry::new(Timestamp::from_second(1_000).unwrap(), HistoryKind::Prompt);
    let recent = HistoryEntry::transition(Timestamp::now(), State::Active, State::Monitoring);
    peer.fake
        .update(|state| state.history = vec![old.clone(), recent.clone()]);

    let all = peer.proxy.history(0).await.unwrap();
    assert_eq!(
        from_json_lines::<HistoryEntry>(&all).unwrap(),
        [old, recent.clone()]
    );
    let hour = peer.proxy.history(3600).await.unwrap();
    assert_eq!(from_json_lines::<HistoryEntry>(&hour).unwrap(), [recent]);

    let since = peer.fake.state().history_since;
    assert_eq!(since[0], Timestamp::MIN);
    let age = Timestamp::now().duration_since(since[1]).as_secs();
    assert!((3600..3660).contains(&age), "{age}");
}

#[tokio::test]
async fn prompt_answers_become_outcomes() {
    let peer = peer().await.unwrap();
    for (kind, minutes) in [
        ("snooze", 15),
        ("cancel", 0),
        ("timeout", 0),
        ("dismissed", 9),
    ] {
        peer.proxy.prompt_answer(kind, minutes).await.unwrap();
    }
    assert_eq!(
        peer.fake.state().answers,
        [
            PromptOutcome::Snooze(mins(15)),
            PromptOutcome::Cancel,
            PromptOutcome::Timeout,
            PromptOutcome::Dismissed,
        ]
    );
}

#[tokio::test]
async fn bad_prompt_answers_are_refused() {
    let peer = peer().await.unwrap();
    let kind = peer.proxy.prompt_answer("later", 5).await.unwrap_err();
    assert_eq!(
        invalid_args(kind).unwrap(),
        r#"invalid prompt answer: unknown kind "later""#
    );
    let zero = peer.proxy.prompt_answer("snooze", 0).await.unwrap_err();
    assert_eq!(
        invalid_args(zero).unwrap(),
        "invalid prompt answer: snooze needs at least 1 minute"
    );
    let long = peer.proxy.prompt_answer("snooze", 721).await.unwrap_err();
    assert_eq!(
        invalid_args(long).unwrap(),
        "snooze must be at most 720 minutes"
    );
    assert_eq!(peer.fake.state().answers, []);
}

#[tokio::test]
async fn pickers_list_outputs_gamepads_and_players() {
    let peer = peer().await.unwrap();
    let pad = GamepadDevice {
        id: "/dev/input/event7".into(),
        name: "8BitDo Pro 2".into(),
        ignored: true,
        last_activity: None,
    };
    peer.fake.update(|state| {
        state.outputs = vec!["HDMI-A-1".into(), "DP-1".into()];
        state.gamepads = vec![pad.clone()];
        state.players = vec![MediaPlayer::with_identity("spotify", "Spotify")];
    });

    assert_eq!(peer.proxy.outputs().await.unwrap(), ["HDMI-A-1", "DP-1"]);
    let pads: Vec<GamepadInfo> = from_json(&peer.proxy.gamepads().await.unwrap()).unwrap();
    assert_eq!(
        pads,
        [GamepadInfo::from_device(&pad, std::time::Instant::now())]
    );
    let players: Vec<PlayerInfo> = from_json(&peer.proxy.players().await.unwrap()).unwrap();
    assert_eq!(
        players,
        [PlayerInfo {
            name: "spotify".into(),
            identity: "Spotify".into(),
        }]
    );
}

#[tokio::test]
async fn daemon_failures_come_back_as_failed() {
    let peer = peer().await.unwrap();
    peer.fake.update(|state| {
        state.failure = Some(BackendError::Disconnected("daemon stopping".into()));
    });
    let message = Ok("disconnected: daemon stopping".to_owned());
    let p = &peer.proxy;
    assert_eq!(failed(p.status().await.unwrap_err()), message);
    assert_eq!(failed(p.snooze(15 * 60).await.unwrap_err()), message);
    assert_eq!(failed(p.cancel_snooze().await.unwrap_err()), message);
    assert_eq!(failed(p.pause().await.unwrap_err()), message);
    assert_eq!(failed(p.resume().await.unwrap_err()), message);
    assert_eq!(failed(p.reload().await.unwrap_err()), message);
    assert_eq!(failed(p.history(0).await.unwrap_err()), message);
    let answer = p.prompt_answer("cancel", 0).await.unwrap_err();
    assert_eq!(failed(answer), message);
    assert_eq!(failed(p.outputs().await.unwrap_err()), message);
    assert_eq!(failed(p.players().await.unwrap_err()), message);
}

#[tokio::test]
async fn daemon_signals_reach_clients() {
    let peer = peer().await.unwrap();
    let mut states = peer.proxy.receive_state_changed().await.unwrap();
    let mut configs = peer.proxy.receive_config_changed().await.unwrap();
    let mut probes = peer.proxy.receive_probe_sample().await.unwrap();
    let signals = peer.service.signals();

    signals.state_changed(State::Blanked).await.unwrap();
    let report = ReloadReport::rejected(vec!["bad".into()]);
    signals.config_changed(&report).await.unwrap();
    let sample = peer.fake.state().sample;
    signals.probe_sample(&sample).await.unwrap();

    let state = timeout(WAIT, states.next()).await.unwrap().unwrap();
    assert_eq!(state.args().unwrap().state(), &"blanked");
    let config = timeout(WAIT, configs.next()).await.unwrap().unwrap();
    let args = config.args().unwrap();
    assert!(!*args.ok());
    assert_eq!(args.errors(), &["bad".to_owned()]);
    let probe = timeout(WAIT, probes.next()).await.unwrap().unwrap();
    let json = probe.args().unwrap().json().to_owned();
    assert_eq!(from_json::<ProbeSample>(json).unwrap(), sample);
}

#[tokio::test]
async fn start_probe_streams_samples_until_stopped() {
    let peer = peer().await.unwrap();
    let mut probes = peer.proxy.receive_probe_sample().await.unwrap();
    peer.proxy.start_probe(100).await.unwrap();

    for _ in 0..2 {
        let probe = timeout(WAIT, probes.next()).await.unwrap().unwrap();
        let json = probe.args().unwrap().json().to_owned();
        assert_eq!(
            from_json::<ProbeSample>(json).unwrap(),
            peer.fake.state().sample
        );
    }
    assert_eq!(
        peer.fake.state().probe_intervals,
        [Duration::from_millis(100)]
    );

    peer.proxy.stop_probe().await.unwrap();
    timeout(WAIT, peer.fake.wait_for_probes(0)).await.unwrap();
}

#[tokio::test]
async fn probe_intervals_below_the_minimum_are_refused() {
    let peer = peer().await.unwrap();
    let err = peer.proxy.start_probe(99).await.unwrap_err();
    assert_eq!(
        invalid_args(err).unwrap(),
        "probe interval must be at least 100 ms"
    );
    assert_eq!(peer.fake.live_probes(), 0);
}

#[tokio::test]
async fn serving_twice_on_one_connection_fails() {
    let peer = peer().await.unwrap();
    let again = Service::serve(
        peer.service.connection().clone(),
        Arc::new(FakeHandle::new()),
    )
    .await
    .unwrap_err();
    assert!(again.to_string().contains("already served"), "{again}");
}
