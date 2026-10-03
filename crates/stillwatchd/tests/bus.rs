//! The control service on a private session bus: the well-known name, probe
//! lifetime tracking through `NameOwnerChanged`, and introspection.

use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt as _;
use stillwatch_core::state::State;
use stillwatch_ipc::proxy::StillwatchProxy;
use stillwatch_ipc::{BUS_NAME, INTERFACE, OBJECT_PATH};
use stillwatch_testkit::PrivateBus;
use stillwatchd::service::fake::FakeHandle;
use stillwatchd::service::{Service, ServiceError};
use tokio::time::timeout;

const WAIT: Duration = Duration::from_secs(5);

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

async fn daemon(bus: &PrivateBus) -> TestResult<(Arc<FakeHandle>, Service)> {
    let fake = Arc::new(FakeHandle::new());
    let conn = bus.connect().await?;
    let service = Service::claim(conn, Arc::clone(&fake) as _).await?;
    Ok((fake, service))
}

async fn client(bus: &PrivateBus) -> TestResult<(zbus::Connection, StillwatchProxy<'static>)> {
    let conn = bus.connect().await?;
    let proxy = StillwatchProxy::new(&conn).await?;
    Ok((conn, proxy))
}

#[tokio::test]
async fn a_second_daemon_is_refused() {
    let Some(bus) = PrivateBus::start().unwrap() else {
        return;
    };
    let (fake, _first) = daemon(&bus).await.unwrap();

    let conn = bus.connect().await.unwrap();
    let err = Service::claim(conn, Arc::new(FakeHandle::new()))
        .await
        .unwrap_err();
    assert!(matches!(err, ServiceError::AlreadyRunning), "{err:?}");
    assert_eq!(
        err.to_string(),
        "another stillwatchd is already running \
         (io.github.jslay88.Stillwatch is taken on the session bus)"
    );

    let (_conn, proxy) = client(&bus).await.unwrap();
    proxy.pause().await.unwrap();
    assert_eq!(fake.state().controls.len(), 1);
}

#[tokio::test]
async fn methods_and_signals_work_by_well_known_name() {
    let Some(bus) = PrivateBus::start().unwrap() else {
        return;
    };
    let (_fake, service) = daemon(&bus).await.unwrap();
    let (_conn, proxy) = client(&bus).await.unwrap();
    assert_eq!(proxy.inner().destination().as_str(), BUS_NAME);

    let mut states = proxy.receive_state_changed().await.unwrap();
    assert!(
        proxy
            .status()
            .await
            .unwrap()
            .contains(r#""state":"active""#)
    );
    service
        .signals()
        .state_changed(State::Prompting)
        .await
        .unwrap();
    let state = timeout(WAIT, states.next()).await.unwrap().unwrap();
    assert_eq!(state.args().unwrap().state(), &"prompting");
}

#[tokio::test]
async fn the_probe_stops_when_its_client_disconnects() {
    let Some(bus) = PrivateBus::start().unwrap() else {
        return;
    };
    let (fake, _service) = daemon(&bus).await.unwrap();
    let (conn, proxy) = client(&bus).await.unwrap();

    proxy.start_probe(100).await.unwrap();
    timeout(WAIT, fake.wait_for_probes(1)).await.unwrap();
    drop(proxy);
    conn.close().await.unwrap();
    timeout(WAIT, fake.wait_for_probes(0)).await.unwrap();
}

#[tokio::test]
async fn the_probe_runs_while_any_client_is_subscribed() {
    let Some(bus) = PrivateBus::start().unwrap() else {
        return;
    };
    let (fake, _service) = daemon(&bus).await.unwrap();
    let (_gui, heatmap) = client(&bus).await.unwrap();
    let (cli_conn, cli) = client(&bus).await.unwrap();

    heatmap.start_probe(1000).await.unwrap();
    cli.start_probe(1000).await.unwrap();
    heatmap.stop_probe().await.unwrap();
    assert_eq!(fake.live_probes(), 1);

    drop(cli);
    cli_conn.close().await.unwrap();
    timeout(WAIT, fake.wait_for_probes(0)).await.unwrap();
    assert_eq!(fake.state().probe_intervals, [Duration::from_secs(1)]);
}

#[tokio::test]
async fn busctl_introspects_the_interface() {
    let Some(bus) = PrivateBus::start().unwrap() else {
        return;
    };
    let (_fake, _service) = daemon(&bus).await.unwrap();
    let output = tokio::process::Command::new("busctl")
        .arg(format!("--address={}", bus.address()))
        .args(["introspect", BUS_NAME, OBJECT_PATH, INTERFACE])
        .output()
        .await;
    let output = match output {
        Ok(output) => output,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("skipping: busctl isn't installed");
            return;
        }
        Err(err) => panic!("busctl failed to start: {err}"),
    };
    assert!(output.status.success(), "{output:?}");
    let listing = String::from_utf8(output.stdout).unwrap();
    for member in [
        ".Status ",
        ".Snooze ",
        ".CancelSnooze ",
        ".Pause ",
        ".Resume ",
        ".Reload ",
        ".History ",
        ".StartProbe ",
        ".StopProbe ",
        ".PromptAnswer ",
        ".Outputs ",
        ".Gamepads ",
        ".Players ",
        ".StateChanged ",
        ".ConfigChanged ",
        ".ProbeSample ",
    ] {
        assert!(listing.contains(member), "{member} missing from\n{listing}");
    }
}
