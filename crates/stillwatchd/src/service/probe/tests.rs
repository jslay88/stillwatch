use std::sync::Arc;
use std::time::Duration;

use zbus::connection::Builder;
use zbus::{Connection, Guid};

use super::*;
use crate::service::fake::FakeHandle;

const SECOND: Duration = Duration::from_secs(1);

async fn peer() -> Connection {
    let (server, client) = tokio::net::UnixStream::pair().unwrap();
    let server = Builder::unix_stream(server)
        .server(Guid::generate())
        .unwrap()
        .p2p()
        .build();
    let client = Builder::unix_stream(client).p2p().build();
    let (server, _client) = tokio::try_join!(server, client).unwrap();
    server
}

fn hub() -> (Arc<FakeHandle>, Arc<ProbeHub>) {
    let fake = Arc::new(FakeHandle::new());
    let hub = ProbeHub::new(Arc::clone(&fake) as Arc<dyn DaemonHandle>);
    (fake, hub)
}

fn clients(hub: &ProbeHub) -> Vec<String> {
    let mut names: Vec<_> = lock(&hub.subscriptions).clients.keys().cloned().collect();
    names.sort();
    names
}

fn interval(hub: &ProbeHub) -> Option<Duration> {
    lock(&hub.subscriptions)
        .running
        .as_ref()
        .map(|run| run.interval)
}

#[tokio::test]
async fn runs_until_the_last_client_stops() {
    let conn = peer().await;
    let (fake, hub) = hub();
    hub.subscribe(&conn, Some(":1.1".into()), SECOND)
        .await
        .unwrap();
    hub.subscribe(&conn, Some(":1.2".into()), SECOND)
        .await
        .unwrap();
    fake.wait_for_probes(1).await;
    assert_eq!(clients(&hub), [":1.1", ":1.2"]);
    assert_eq!(fake.state().probe_intervals, [SECOND]);

    hub.unsubscribe(":1.1");
    assert_eq!(interval(&hub), Some(SECOND));
    hub.unsubscribe(":1.2");
    assert_eq!(interval(&hub), None);
    fake.wait_for_probes(0).await;
}

#[tokio::test]
async fn a_new_interval_restarts_the_probe() {
    let conn = peer().await;
    let (fake, hub) = hub();
    hub.subscribe(&conn, None, SECOND).await.unwrap();
    hub.subscribe(&conn, None, SECOND).await.unwrap();
    hub.subscribe(&conn, None, 2 * SECOND).await.unwrap();
    assert_eq!(clients(&hub), [""]);
    assert_eq!(interval(&hub), Some(2 * SECOND));
    assert_eq!(fake.state().probe_intervals, [SECOND, 2 * SECOND]);
    fake.wait_for_probes(1).await;
}

#[tokio::test]
async fn stopping_without_subscribing_changes_nothing() {
    let conn = peer().await;
    let (fake, hub) = hub();
    hub.unsubscribe(":1.9");
    assert_eq!(interval(&hub), None);

    hub.subscribe(&conn, Some(":1.1".into()), SECOND)
        .await
        .unwrap();
    hub.unsubscribe(":1.9");
    assert_eq!(clients(&hub), [":1.1"]);
    assert_eq!(fake.live_probes(), 1);
}
