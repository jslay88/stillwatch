//! Forced reloads: SIGHUP (`systemctl --user reload stillwatch`) and D-Bus
//! `Reload()` reload even when the file hasn't changed, and every attempt
//! emits `ConfigChanged` exactly once.

use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt as _;
use rustix::process::{Signal as RawSignal, getpid, kill_process};
use stillwatch_ipc::proxy::StillwatchProxy;
use stillwatch_testkit::PrivateBus;
use stillwatchd::config_watch::{ReloadOutcome, ReloadTrigger, Reloader, reload_and_report};
use stillwatchd::service::Service;
use stillwatchd::service::fake::FakeHandle;
use stillwatchd::signals::{Signal, SignalSource as _, Signals};
use tokio::time::timeout;

const WAIT: Duration = Duration::from_secs(5);

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

fn reloader(contents: &str) -> TestResult<(tempfile::TempDir, Reloader)> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("config.toml");
    std::fs::write(&path, contents)?;
    let (reloader, _) = Reloader::load(path)?;
    Ok((dir, reloader))
}

#[tokio::test]
async fn sighup_forces_a_reload_of_an_unchanged_file() {
    let (_dir, mut reloader) = reloader("[stale]\nstale_percent = 50\n").unwrap();
    let mut signals = Signals::install().unwrap();
    kill_process(getpid(), RawSignal::HUP).unwrap();

    let signal = timeout(WAIT, signals.recv()).await.unwrap().unwrap();
    assert_eq!(signal, Signal::Hangup);
    let trigger = signal.reload_trigger().unwrap();
    assert!(matches!(
        reloader.reload(ReloadTrigger::FileChanged),
        ReloadOutcome::Unchanged
    ));
    let ReloadOutcome::Applied(applied) = reloader.reload(trigger) else {
        panic!("SIGHUP didn't reload");
    };
    assert!(applied.changes.is_empty());
    assert_eq!(applied.loaded.config.stale.stale_percent, 50);
}

#[tokio::test]
async fn dbus_reload_emits_config_changed_once_per_attempt() {
    let Some(bus) = PrivateBus::start().unwrap() else {
        return;
    };
    let service = Service::claim(bus.connect().await.unwrap(), Arc::new(FakeHandle::new()))
        .await
        .unwrap();
    let client = bus.connect().await.unwrap();
    let proxy = StillwatchProxy::new(&client).await.unwrap();
    let mut changes = proxy.receive_config_changed().await.unwrap();
    let (dir, mut reloader) = reloader("[stale]\nstale_percent = 50\n").unwrap();

    let outcome =
        reload_and_report(&mut reloader, ReloadTrigger::Requested, service.signals()).await;
    assert!(matches!(outcome, ReloadOutcome::Applied(_)));
    let signal = timeout(WAIT, changes.next()).await.unwrap().unwrap();
    let args = signal.args().unwrap();
    assert!(*args.ok());
    assert_eq!(args.errors(), &[] as &[String]);

    std::fs::write(
        dir.path().join("config.toml"),
        "[stale]\nstale_percent = 0\n",
    )
    .unwrap();
    let outcome =
        reload_and_report(&mut reloader, ReloadTrigger::Requested, service.signals()).await;
    assert!(matches!(outcome, ReloadOutcome::Rejected(_)));
    let signal = timeout(WAIT, changes.next()).await.unwrap().unwrap();
    let args = signal.args().unwrap();
    assert!(!*args.ok());
    assert_eq!(args.errors(), &reloader.errors().to_vec());
    assert_eq!(outcome.report().unwrap().errors, reloader.errors());

    let outcome =
        reload_and_report(&mut reloader, ReloadTrigger::FileChanged, service.signals()).await;
    assert!(matches!(outcome, ReloadOutcome::Unchanged));
    assert!(
        timeout(Duration::from_millis(300), changes.next())
            .await
            .is_err(),
        "an unchanged file emitted ConfigChanged"
    );
}
