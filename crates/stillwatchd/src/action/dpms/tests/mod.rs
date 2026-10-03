//! The DPMS watch and blanker against a fake compositor and a scripted
//! command runner.

mod compositor;

use std::collections::VecDeque;
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use compositor::{DpmsCompositor, MANAGER, OFF, ON, OUTPUT, STANDBY};
use stillwatch_core::backend::{BackendError, Blanker};
use stillwatch_core::event::Event;
use stillwatch_core::mocks::RecordingSink;
use tokio::time::timeout;
use wayland_client::Connection;

use super::DpmsBlanker;
use crate::process::CommandError;
use crate::process::scripted::ScriptedRunner;
const LIMIT: Duration = Duration::from_secs(5);

const HDMI: (u32, &str) = (66, "HDMI-A-1");
const DP: (u32, &str) = (67, "DP-1");

fn kwin() -> Vec<(u32, &'static str, u32)> {
    vec![
        (1, "wl_compositor", 6),
        (HDMI.0, OUTPUT, 4),
        (DP.0, OUTPUT, 4),
        (29, MANAGER, 1),
    ]
}

fn power(output: &str, on: bool) -> Event {
    Event::DisplayPower {
        output: output.to_owned(),
        on,
    }
}

/// Connects to each socket in turn, then reports no compositor.
fn connector(
    sockets: Vec<UnixStream>,
) -> impl Fn() -> Result<Connection, BackendError> + Send + Sync + 'static {
    let sockets = Mutex::new(VecDeque::from(sockets));
    move || {
        let socket = sockets
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| BackendError::Disconnected("no compositor".into()))?;
        Ok(Connection::from_socket(socket).unwrap())
    }
}

/// Runs `script` as a compositor with `names` on its own thread.
fn serve<T: Send + 'static>(
    names: &[(u32, &str)],
    script: impl FnOnce(&mut DpmsCompositor) -> std::io::Result<T> + Send + 'static,
) -> (JoinHandle<T>, UnixStream) {
    let (mut server, client) = DpmsCompositor::pair(names);
    (thread::spawn(move || script(&mut server).unwrap()), client)
}

fn blanker(sockets: Vec<UnixStream>, runner: &Arc<ScriptedRunner>) -> DpmsBlanker {
    DpmsBlanker::new()
        .with_connector(connector(sockets))
        .with_runner(runner.clone())
}

async fn watch_once(blanker: &DpmsBlanker, sink: &Arc<RecordingSink>) -> BackendError {
    timeout(LIMIT, blanker.watch(sink.clone()))
        .await
        .unwrap()
        .unwrap_err()
}

#[tokio::test]
async fn reports_each_outputs_first_state_then_only_changes() {
    let (server, client) = serve(&[HDMI, DP], |c| {
        c.fake.advertise(&kwin())?;
        c.serve_dpms(2)?;
        c.mode(HDMI.0, OFF)?;
        c.mode(HDMI.0, OFF)?;
        c.mode(HDMI.0, STANDBY)?;
        c.mode(DP.0, 7)?;
        c.mode(HDMI.0, ON)?;
        Ok(c.versions.clone())
    });
    let sink = Arc::new(RecordingSink::new());
    let runner = Arc::new(ScriptedRunner::new());
    let error = watch_once(&blanker(vec![client], &runner), &sink).await;

    assert!(matches!(error, BackendError::Disconnected(_)), "{error}");
    assert_eq!(
        sink.events(),
        [
            power(HDMI.1, true),
            power(DP.1, true),
            power(HDMI.1, false),
            power(HDMI.1, true),
        ]
    );
    let versions = server.join().unwrap();
    assert_eq!(versions[&HDMI.0], 4);
    assert_eq!(versions[&29], 1);
    assert_eq!(runner.calls(), []);
}

#[tokio::test]
async fn follows_outputs_plugged_in_and_removed() {
    let manager_first = vec![(29, MANAGER, 1), (HDMI.0, OUTPUT, 4)];
    let (server, client) = serve(&[HDMI], move |c| {
        c.fake.advertise(&manager_first)?;
        c.serve_dpms(1)?;
        c.name(DP.0, DP.1);
        c.starts(DP.0, OFF);
        c.fake.global(DP.0, OUTPUT, 5)?;
        c.serve_dpms(2)?;
        c.fake.remove_global(HDMI.0)?;
        c.serve_until(|c| c.released_dpms == [HDMI.0] && c.released_outputs == [HDMI.0])?;
        c.name(70, HDMI.1);
        c.fake.global(70, OUTPUT, 4)?;
        c.serve_dpms(3)?;
        Ok(c.versions[&DP.0])
    });
    let sink = Arc::new(RecordingSink::new());
    let error = watch_once(&blanker(vec![client], &Arc::default()), &sink).await;

    assert!(error.is_transient(), "{error}");
    assert_eq!(
        sink.events(),
        [power(HDMI.1, true), power(DP.1, false), power(HDMI.1, true)]
    );
    assert_eq!(server.join().unwrap(), 4, "bound at the version we know");
}

#[tokio::test]
async fn outputs_without_a_name_are_not_reported() {
    let globals = vec![(HDMI.0, OUTPUT, 2), (DP.0, OUTPUT, 4), (29, MANAGER, 1)];
    let (server, client) = serve(&[DP], move |c| {
        c.fake.advertise(&globals)?;
        c.serve_dpms(2)?;
        c.mode(HDMI.0, OFF)?;
        c.fake.remove_global(HDMI.0)?;
        c.serve_until(|c| c.released_dpms == [HDMI.0])?;
        Ok(c.released_outputs.clone())
    });
    let sink = Arc::new(RecordingSink::new());
    let error = watch_once(&blanker(vec![client], &Arc::default()), &sink).await;

    assert!(error.is_transient(), "{error}");
    assert_eq!(sink.events(), [power(DP.1, true)]);
    assert!(server.join().unwrap().is_empty(), "v2 can't be released");
}

#[tokio::test]
async fn a_compositor_without_the_dpms_manager_is_unavailable() {
    let (server, client) = serve(&[HDMI], |c| {
        c.fake.advertise(&[(HDMI.0, OUTPUT, 4)])?;
        c.fake.wait_for_close();
        Ok(())
    });
    let sink = Arc::new(RecordingSink::new());
    let error = watch_once(&blanker(vec![client], &Arc::default()), &sink).await;
    assert!(
        matches!(&error, BackendError::Unavailable(m) if m.contains(MANAGER)),
        "{error}"
    );
    server.join().unwrap();
}

#[tokio::test]
async fn a_withdrawn_manager_is_a_disconnect() {
    let (server, client) = serve(&[HDMI, DP], |c| {
        c.fake.advertise(&kwin())?;
        c.serve_dpms(2)?;
        c.fake.remove_global(29)?;
        c.fake.wait_for_close();
        Ok(())
    });
    let sink = Arc::new(RecordingSink::new());
    let error = watch_once(&blanker(vec![client], &Arc::default()), &sink).await;
    assert!(
        matches!(&error, BackendError::Disconnected(m) if m.contains("withdrew")),
        "{error}"
    );
    server.join().unwrap();
}

#[tokio::test]
async fn protocol_errors_end_the_watch() {
    let (server, client) = serve(&[HDMI], |c| {
        c.fake.advertise(&kwin())?;
        c.serve_dpms(1)?;
        let id = c.dpms[&HDMI.0];
        c.fake.protocol_error(id, "bad mode")?;
        c.fake.wait_for_close();
        Ok(())
    });
    let sink = Arc::new(RecordingSink::new());
    let error = watch_once(&blanker(vec![client], &Arc::default()), &sink).await;
    assert!(
        matches!(&error, BackendError::Protocol(m) if m.contains("bad mode")),
        "{error}"
    );
    server.join().unwrap();
}

#[tokio::test]
async fn no_compositor_is_a_transient_error() {
    let sink = Arc::new(RecordingSink::new());
    let error = watch_once(&blanker(Vec::new(), &Arc::default()), &sink).await;
    assert!(error.is_transient(), "{error}");
}

/// A compositor that answers one output listing.
fn listing(names: &[(u32, &str)]) -> (JoinHandle<()>, UnixStream) {
    serve(names, |c| {
        c.fake.advertise(&kwin())?;
        c.serve_until_closed();
        Ok(())
    })
}

#[tokio::test]
async fn blanking_some_outputs_excludes_the_rest() {
    let (server, client) = listing(&[HDMI, DP]);
    let runner = Arc::new(ScriptedRunner::new());
    let blanker = blanker(vec![client], &runner);
    timeout(LIMIT, blanker.blank(&[HDMI.1.to_owned()]))
        .await
        .unwrap()
        .unwrap();
    server.join().unwrap();

    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].to_string(),
        "kscreen-doctor --dpms off --dpms-excluded DP-1"
    );
}

#[tokio::test]
async fn an_empty_list_switches_everything_without_listing() {
    let runner = Arc::new(ScriptedRunner::new());
    let blanker = blanker(Vec::new(), &runner);
    blanker.blank(&[]).await.unwrap();
    blanker.unblank(&[]).await.unwrap();
    let commands: Vec<String> = runner.calls().iter().map(ToString::to_string).collect();
    assert_eq!(
        commands,
        ["kscreen-doctor --dpms off", "kscreen-doctor --dpms on"]
    );
}

#[tokio::test]
async fn disconnected_targets_fail_a_blank_and_skip_an_unblank() {
    let (first, first_client) = listing(&[HDMI]);
    let (second, second_client) = listing(&[HDMI]);
    let runner = Arc::new(ScriptedRunner::new());
    let blanker = blanker(vec![first_client, second_client], &runner);
    let gone = [DP.1.to_owned()];

    let error = blanker.blank(&gone).await.unwrap_err();
    assert!(matches!(error, BackendError::NotFound(_)), "{error}");
    assert_eq!(blanker.unblank(&gone).await, Ok(()));
    assert_eq!(runner.calls(), []);
    first.join().unwrap();
    second.join().unwrap();
}

#[tokio::test]
async fn kscreen_doctor_failures_are_typed_errors() {
    let runner = Arc::new(ScriptedRunner::new());
    runner.push_error(CommandError::NotFound {
        program: "kscreen-doctor".into(),
    });
    runner.push_error(CommandError::Failed {
        program: "kscreen-doctor".into(),
        code: Some(1),
        stderr: "Failed to create wl_display".into(),
    });
    runner.push_error(CommandError::TimedOut {
        program: "kscreen-doctor".into(),
        after: Duration::from_secs(10),
    });
    runner.push_stderr("DPMS not supported in this system");
    let blanker = blanker(Vec::new(), &runner);

    let missing = blanker.blank(&[]).await.unwrap_err();
    assert!(matches!(missing, BackendError::Unavailable(_)), "{missing}");
    let failed = blanker.blank(&[]).await.unwrap_err();
    assert!(
        matches!(&failed, BackendError::Protocol(m) if m.contains("wl_display")),
        "{failed}"
    );
    let slow = blanker.unblank(&[]).await.unwrap_err();
    assert!(matches!(slow, BackendError::Io(_)), "{slow}");
    let refused = blanker.blank(&[]).await.unwrap_err();
    assert!(matches!(refused, BackendError::Unsupported(_)), "{refused}");
}

#[tokio::test]
async fn listing_failures_stop_before_running_anything() {
    let runner = Arc::new(ScriptedRunner::new());
    let blanker = blanker(Vec::new(), &runner);
    let error = blanker.blank(&[HDMI.1.to_owned()]).await.unwrap_err();
    assert!(error.is_transient(), "{error}");
    assert_eq!(runner.calls(), []);
}

#[test]
fn builder_options_reach_the_command() {
    let runner = Arc::new(ScriptedRunner::new());
    let blanker = DpmsBlanker::default()
        .on_display("/nonexistent/stillwatch-test-0")
        .with_program("/usr/bin/kscreen-doctor")
        .with_timeout(Duration::from_secs(3))
        .with_runner(runner.clone());
    let spec = blanker.invocation.command(super::Power::On, &[]);
    assert_eq!(spec.program, "/usr/bin/kscreen-doctor");
    assert_eq!(spec.timeout, Duration::from_secs(3));
    assert!(spec.env.contains(&(
        "WAYLAND_DISPLAY".into(),
        "/nonexistent/stillwatch-test-0".into()
    )));
    let error = (blanker.connect)().unwrap_err();
    assert!(error.is_transient(), "{error}");
}
