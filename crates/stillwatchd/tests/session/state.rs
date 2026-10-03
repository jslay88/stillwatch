//! `is_locked`, the state a watch starts from, and finding the session.

use stillwatch_core::backend::SessionMonitor;
use stillwatch_core::event::SessionEvent::{Locked, PrepareForSleep, Unlocked};
use stillwatch_testkit::logind::Options;

use crate::harness::{Desktop, TestResult, Watch};

#[tokio::test]
async fn starting_locked_is_reported_by_is_locked_not_the_watch() -> TestResult {
    let options = Options {
        locked_hint: true,
        ..Options::default()
    };
    let Some(desktop) = Desktop::start(options, true).await? else {
        return Ok(());
    };
    let monitor = desktop.monitor();
    assert!(monitor.is_locked().await?);
    let mut watch = Watch::start(&desktop, &monitor).await?;
    watch.assert_quiet().await?;
    desktop.logind.session().set_locked_hint(false).await?;
    assert_eq!(watch.next().await?, Unlocked);
    assert!(!monitor.is_locked().await?);
    Ok(())
}

#[tokio::test]
async fn an_active_screensaver_counts_as_locked() -> TestResult {
    let Some(desktop) = Desktop::start(Options::default(), true).await? else {
        return Ok(());
    };
    let monitor = desktop.monitor();
    assert!(!monitor.is_locked().await?);
    desktop.screensaver()?.set_active(true).await?;
    assert!(monitor.is_locked().await?);
    Ok(())
}

#[tokio::test]
async fn a_lock_between_is_locked_and_the_watch_is_not_lost() -> TestResult {
    let Some(desktop) = Desktop::start(Options::default(), true).await? else {
        return Ok(());
    };
    let monitor = desktop.monitor();
    assert!(!monitor.is_locked().await?);
    desktop.logind.session().set_locked_hint(true).await?;
    let mut watch = Watch::start(&desktop, &monitor).await?;
    assert_eq!(watch.next().await?, Locked);
    watch.assert_quiet().await?;
    Ok(())
}

#[tokio::test]
async fn a_restarted_watch_reports_what_it_missed() -> TestResult {
    let Some(desktop) = Desktop::start(Options::default(), true).await? else {
        return Ok(());
    };
    let monitor = desktop.monitor();
    let mut watch = Watch::start(&desktop, &monitor).await?;
    watch.assert_quiet().await?;
    drop(watch);

    desktop.screensaver()?.set_active(true).await?;
    desktop.logind.prepare_for_sleep(true).await?;
    let mut watch = Watch::start(&desktop, &monitor).await?;
    assert_eq!(watch.next().await?, Locked);
    assert_eq!(watch.next().await?, PrepareForSleep);
    watch.assert_quiet().await?;
    Ok(())
}

#[tokio::test]
async fn the_session_is_found_by_id_then_pid_then_auto() -> TestResult {
    let Some(desktop) = Desktop::start(Options::default(), false).await? else {
        return Ok(());
    };
    let monitor = desktop.unshared_monitor().with_session_id("3");
    assert!(!monitor.is_locked().await?);
    assert_eq!(desktop.logind.calls(), ["GetSession 3"]);

    let Some(desktop) = Desktop::start(Options::default(), false).await? else {
        return Ok(());
    };
    let monitor = desktop.unshared_monitor().with_session_id("9").with_pid(42);
    assert!(!monitor.is_locked().await?);
    assert_eq!(
        desktop.logind.calls(),
        ["GetSession 9", "GetSessionByPID 42"]
    );

    let options = Options {
        pid_in_session: false,
        ..Options::default()
    };
    let Some(desktop) = Desktop::start(options, false).await? else {
        return Ok(());
    };
    let monitor = desktop.unshared_monitor().with_pid(42);
    assert!(!monitor.is_locked().await?);
    assert_eq!(
        desktop.logind.calls(),
        ["GetSessionByPID 42", "GetSession auto"]
    );
    Ok(())
}
