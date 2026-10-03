//! Drives the proxy against a stub server over a peer-to-peer connection, so
//! a typo in a member name or signature fails here instead of at runtime.

// zbus generates signal emitter helpers without doc comments.
#![allow(missing_docs)]

use std::sync::{Arc, Mutex};

use futures_util::StreamExt;
use stillwatch_ipc::OBJECT_PATH;
use stillwatch_ipc::proxy::StillwatchProxy;
use zbus::connection::Builder;
use zbus::object_server::SignalEmitter;
use zbus::{Connection, Guid};

#[derive(Clone, Default)]
struct Stub {
    calls: Arc<Mutex<Vec<String>>>,
}

impl Stub {
    fn log(&self, call: impl Into<String>) {
        if let Ok(mut calls) = self.calls.lock() {
            calls.push(call.into());
        }
    }
}

#[zbus::interface(name = "io.github.jslay88.Stillwatch1")]
impl Stub {
    fn status(&self) -> String {
        self.log("Status");
        r#"{"state":"active","state_seconds":1}"#.into()
    }

    fn snooze(&self, seconds: u64) {
        self.log(format!("Snooze {seconds}"));
    }

    fn cancel_snooze(&self) {
        self.log("CancelSnooze");
    }

    fn pause(&self) {
        self.log("Pause");
    }

    fn resume(&self) {
        self.log("Resume");
    }

    fn reload(&self) -> (bool, Vec<String>) {
        self.log("Reload");
        (false, vec!["stale.stale_percent: must be 1-100".into()])
    }

    fn history(&self, since_seconds: u64) -> String {
        self.log(format!("History {since_seconds}"));
        String::new()
    }

    fn start_probe(&self, interval_ms: u32) {
        self.log(format!("StartProbe {interval_ms}"));
    }

    fn stop_probe(&self) {
        self.log("StopProbe");
    }

    fn prompt_answer(&self, kind: &str, minutes: u32) {
        self.log(format!("PromptAnswer {kind} {minutes}"));
    }

    fn outputs(&self) -> Vec<String> {
        self.log("Outputs");
        vec!["HDMI-A-1".into()]
    }

    fn gamepads(&self) -> String {
        self.log("Gamepads");
        "[]".into()
    }

    fn players(&self) -> Vec<String> {
        self.log("Players");
        vec!["spotify".into()]
    }

    #[zbus(signal)]
    async fn state_changed(emitter: &SignalEmitter<'_>, state: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn config_changed(
        emitter: &SignalEmitter<'_>,
        ok: bool,
        errors: Vec<String>,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn probe_sample(emitter: &SignalEmitter<'_>, json: &str) -> zbus::Result<()>;
}

async fn connect(stub: Stub) -> zbus::Result<(Connection, Connection)> {
    let (server, client) = tokio::net::UnixStream::pair()?;
    let server = Builder::unix_stream(server)
        .server(Guid::generate())?
        .p2p()
        .serve_at(OBJECT_PATH, stub)?
        .build();
    let client = Builder::unix_stream(client).p2p().build();
    tokio::try_join!(server, client)
}

#[tokio::test]
async fn every_method_reaches_the_server() {
    let stub = Stub::default();
    let (_server, client) = connect(stub.clone()).await.unwrap();
    let proxy = StillwatchProxy::new(&client).await.unwrap();

    let status: stillwatch_ipc::status::StatusPayload =
        stillwatch_ipc::json::from_json(&proxy.status().await.unwrap()).unwrap();
    assert_eq!(status.state_seconds, 1);
    proxy.snooze(2700).await.unwrap();
    proxy.cancel_snooze().await.unwrap();
    proxy.pause().await.unwrap();
    proxy.resume().await.unwrap();
    assert_eq!(
        proxy.reload().await.unwrap(),
        (false, vec!["stale.stale_percent: must be 1-100".to_owned()])
    );
    assert_eq!(proxy.history(7200).await.unwrap(), "");
    proxy.start_probe(1000).await.unwrap();
    proxy.stop_probe().await.unwrap();
    proxy.prompt_answer("snooze", 45).await.unwrap();
    assert_eq!(proxy.outputs().await.unwrap(), vec!["HDMI-A-1".to_owned()]);
    assert_eq!(proxy.gamepads().await.unwrap(), "[]");
    assert_eq!(proxy.players().await.unwrap(), vec!["spotify".to_owned()]);

    assert_eq!(
        *stub.calls.lock().unwrap(),
        [
            "Status",
            "Snooze 2700",
            "CancelSnooze",
            "Pause",
            "Resume",
            "Reload",
            "History 7200",
            "StartProbe 1000",
            "StopProbe",
            "PromptAnswer snooze 45",
            "Outputs",
            "Gamepads",
            "Players",
        ]
    );
}

#[tokio::test]
async fn signals_reach_the_client() {
    let (server, client) = connect(Stub::default()).await.unwrap();
    let proxy = StillwatchProxy::new(&client).await.unwrap();
    let mut states = proxy.receive_state_changed().await.unwrap();
    let mut configs = proxy.receive_config_changed().await.unwrap();
    let mut probes = proxy.receive_probe_sample().await.unwrap();

    let iface = server
        .object_server()
        .interface::<_, Stub>(OBJECT_PATH)
        .await
        .unwrap();
    let emitter = iface.signal_emitter();
    Stub::state_changed(emitter, "blanked").await.unwrap();
    Stub::config_changed(emitter, true, Vec::new())
        .await
        .unwrap();
    Stub::probe_sample(emitter, "{}").await.unwrap();

    let state = states.next().await.unwrap();
    assert_eq!(state.args().unwrap().state(), &"blanked");
    let config = configs.next().await.unwrap();
    let args = config.args().unwrap();
    assert!(*args.ok());
    assert_eq!(args.errors(), &Vec::<String>::new());
    let probe = probes.next().await.unwrap();
    assert_eq!(probe.args().unwrap().json(), &"{}");
}
