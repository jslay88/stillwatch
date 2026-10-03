//! Lock, unlock, and suspend events from each source, and duplicates.

use stillwatch_core::event::SessionEvent::{Locked, PrepareForSleep, ResumedFromSleep, Unlocked};
use stillwatch_testkit::logind::Options;

use crate::harness::{Desktop, TestResult, Watch};

#[tokio::test]
async fn locked_hint_alone_reports_lock_and_unlock() -> TestResult {
    let Some(desktop) = Desktop::start(Options::default(), false).await? else {
        return Ok(());
    };
    let mut watch = Watch::start(&desktop, &desktop.monitor()).await?;
    let session = desktop.logind.session();
    session.set_locked_hint(true).await?;
    assert_eq!(watch.next().await?, Locked);
    session.set_locked_hint(true).await?;
    watch.assert_quiet().await?;
    session.set_locked_hint(false).await?;
    assert_eq!(watch.next().await?, Unlocked);
    session.invalidate_locked_hint(true).await?;
    assert_eq!(watch.next().await?, Locked);
    Ok(())
}

#[tokio::test]
async fn the_screensaver_alone_reports_lock_and_unlock() -> TestResult {
    let Some(desktop) = Desktop::start(Options::default(), true).await? else {
        return Ok(());
    };
    let mut watch = Watch::start(&desktop, &desktop.monitor()).await?;
    let screensaver = desktop.screensaver()?;
    screensaver.set_active(true).await?;
    assert_eq!(watch.next().await?, Locked);
    screensaver.set_active(true).await?;
    watch.assert_quiet().await?;
    screensaver.set_active(false).await?;
    assert_eq!(watch.next().await?, Unlocked);
    watch.assert_quiet().await?;
    Ok(())
}

#[tokio::test]
async fn logind_lock_and_unlock_requests_are_reported() -> TestResult {
    let Some(desktop) = Desktop::start(Options::default(), true).await? else {
        return Ok(());
    };
    let mut watch = Watch::start(&desktop, &desktop.monitor()).await?;
    let session = desktop.logind.session();
    session.emit_lock().await?;
    assert_eq!(watch.next().await?, Locked);
    session.emit_unlock().await?;
    assert_eq!(watch.next().await?, Unlocked);
    session.emit_lock().await?;
    assert_eq!(watch.next().await?, Locked);
    Ok(())
}

#[tokio::test]
async fn one_lock_seen_by_every_source_is_reported_once() -> TestResult {
    let Some(desktop) = Desktop::start(Options::default(), true).await? else {
        return Ok(());
    };
    let mut watch = Watch::start(&desktop, &desktop.monitor()).await?;
    let session = desktop.logind.session();
    let screensaver = desktop.screensaver()?;

    session.emit_lock().await?;
    screensaver.set_active(true).await?;
    session.set_locked_hint(true).await?;
    screensaver.set_active(true).await?;
    assert_eq!(watch.next().await?, Locked);
    watch.assert_quiet().await?;

    session.set_locked_hint(false).await?;
    session.emit_unlock().await?;
    screensaver.set_active(false).await?;
    session.set_locked_hint(false).await?;
    assert_eq!(watch.next().await?, Unlocked);
    watch.assert_quiet().await?;
    Ok(())
}

#[tokio::test]
async fn prepare_for_sleep_true_then_false_brackets_a_suspend() -> TestResult {
    let Some(desktop) = Desktop::start(Options::default(), true).await? else {
        return Ok(());
    };
    let mut watch = Watch::start(&desktop, &desktop.monitor()).await?;
    desktop.logind.prepare_for_sleep(true).await?;
    assert_eq!(watch.next().await?, PrepareForSleep);
    desktop.logind.prepare_for_sleep(true).await?;
    watch.assert_quiet().await?;
    desktop.logind.prepare_for_sleep(false).await?;
    assert_eq!(watch.next().await?, ResumedFromSleep);
    desktop.logind.prepare_for_sleep(false).await?;
    watch.assert_quiet().await?;
    Ok(())
}

#[tokio::test]
async fn a_watch_started_mid_suspend_reports_it() -> TestResult {
    let options = Options {
        preparing_for_sleep: true,
        ..Options::default()
    };
    let Some(desktop) = Desktop::start(options, true).await? else {
        return Ok(());
    };
    let mut watch = Watch::start(&desktop, &desktop.monitor()).await?;
    assert_eq!(watch.next().await?, PrepareForSleep);
    desktop.logind.prepare_for_sleep(false).await?;
    assert_eq!(watch.next().await?, ResumedFromSleep);
    Ok(())
}
