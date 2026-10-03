//! Missing services and lost buses.

use std::sync::Arc;

use stillwatch_core::backend::{BackendError, EventSink, SessionMonitor};
use stillwatch_testkit::PrivateBus;
use stillwatch_testkit::logind::Options;
use stillwatchd::session::DbusSessionMonitor;

use crate::harness::{Desktop, TestResult, Watch};

#[tokio::test]
async fn without_logind_or_a_screensaver_everything_is_unavailable() -> TestResult {
    let (Some(system), Some(session)) = (PrivateBus::start()?, PrivateBus::start()?) else {
        return Ok(());
    };
    let monitor = DbusSessionMonitor::at_addresses(system.address(), session.address());
    let locked = monitor.lock().await;
    assert!(
        matches!(locked, Err(BackendError::Unavailable(_))),
        "{locked:?}"
    );
    let read = monitor.is_locked().await;
    assert!(
        matches!(read, Err(BackendError::Unavailable(_))),
        "{read:?}"
    );
    let sink: Arc<dyn EventSink> = Arc::new(|_event| {});
    let watched = monitor.watch(sink).await;
    assert!(
        matches!(watched, Err(BackendError::Unavailable(_))),
        "{watched:?}"
    );
    Ok(())
}

#[tokio::test]
async fn losing_the_system_bus_ends_the_watch_with_a_transient_error() -> TestResult {
    let Some(mut desktop) = Desktop::start(Options::default(), true).await? else {
        return Ok(());
    };
    let watch = Watch::start(&desktop, &desktop.monitor()).await?;
    desktop.system.stop();
    let err = watch.ended().await?;
    assert!(err.is_transient(), "{err}");
    Ok(())
}

#[tokio::test]
async fn losing_the_session_bus_ends_the_watch_with_a_transient_error() -> TestResult {
    let Some(mut desktop) = Desktop::start(Options::default(), true).await? else {
        return Ok(());
    };
    let watch = Watch::start(&desktop, &desktop.monitor()).await?;
    desktop.session.stop();
    let err = watch.ended().await?;
    assert!(err.is_transient(), "{err}");
    Ok(())
}

#[tokio::test]
async fn unreachable_buses_are_transient_errors() {
    let monitor = DbusSessionMonitor::at_addresses(
        "unix:path=/nonexistent/stillwatch/system",
        "unix:path=/nonexistent/stillwatch/session",
    );
    let sink: Arc<dyn EventSink> = Arc::new(|_event| {});
    let err = monitor.watch(sink).await.err();
    assert!(
        err.as_ref().is_some_and(BackendError::is_transient),
        "{err:?}"
    );
    let err = monitor.is_locked().await.err();
    assert!(
        err.as_ref().is_some_and(BackendError::is_transient),
        "{err:?}"
    );
    let err = monitor.lock().await.err();
    assert!(
        err.as_ref().is_some_and(BackendError::is_transient),
        "{err:?}"
    );
}
