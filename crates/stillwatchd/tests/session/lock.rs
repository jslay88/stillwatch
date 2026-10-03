//! `lock()`: the screensaver first, then logind.

use stillwatch_core::backend::SessionMonitor;
use stillwatch_core::event::SessionEvent::Locked;
use stillwatch_testkit::logind::Options;

use crate::harness::{Desktop, TestResult, Watch};

#[tokio::test]
async fn lock_prefers_the_screensaver() -> TestResult {
    let Some(desktop) = Desktop::start(Options::default(), true).await? else {
        return Ok(());
    };
    let monitor = desktop.monitor();
    let mut watch = Watch::start(&desktop, &monitor).await?;
    monitor.lock().await?;
    assert_eq!(watch.next().await?, Locked);
    let screensaver = desktop.screensaver()?;
    assert_eq!(screensaver.lock_calls(), 1);
    assert!(screensaver.is_active());
    assert!(
        !desktop
            .logind
            .calls()
            .iter()
            .any(|call| call.starts_with("LockSession"))
    );
    Ok(())
}

#[tokio::test]
async fn lock_falls_back_to_logind_without_a_screensaver() -> TestResult {
    let Some(desktop) = Desktop::start(Options::default(), false).await? else {
        return Ok(());
    };
    let monitor = desktop.monitor();
    let mut watch = Watch::start(&desktop, &monitor).await?;
    monitor.lock().await?;
    assert_eq!(watch.next().await?, Locked);
    assert!(desktop.logind.calls().contains(&"LockSession 3".to_owned()));
    Ok(())
}

#[tokio::test]
async fn lock_falls_back_to_logind_when_the_screensaver_refuses() -> TestResult {
    let Some(desktop) = Desktop::start(Options::default(), true).await? else {
        return Ok(());
    };
    let screensaver = desktop.screensaver()?;
    screensaver.refuse_lock(true);
    desktop.monitor().lock().await?;
    assert_eq!(screensaver.lock_calls(), 1);
    assert!(!screensaver.is_active());
    assert!(desktop.logind.calls().contains(&"LockSession 3".to_owned()));
    Ok(())
}
