//! The notification prompter against a fake notification server on a private
//! session bus.

#[path = "notification/support.rs"]
mod support;

use std::time::Duration;

use stillwatch_core::backend::{BackendError, Prompter};
use stillwatch_core::config::PromptUrgency;
use stillwatch_core::prompt::{PromptOutcome, Reminder};
use stillwatch_testkit::PrivateBus;
use stillwatch_testkit::notifications::{CLOSED_DISMISSED, FakeNotificationServer};
use stillwatchd::prompt::NotificationPrompter;
use tokio::time::{sleep, timeout};

use self::support::{APP_ID, FAST, Setup, TICK, TestResult, outcome, request, wait_until};

#[tokio::test]
async fn the_prompt_offers_presets_custom_and_blank_now() -> TestResult {
    let Some(s) = Setup::start().await? else {
        return Ok(());
    };
    let (task, sent) = s.shown().await?;
    assert_eq!(sent.app_name, APP_ID);
    assert_eq!(sent.app_icon, APP_ID);
    assert_eq!(sent.replaces_id, 0);
    assert_eq!(sent.summary, "Blanking the screen in 1 min");
    assert!(
        sent.body
            .starts_with("HDMI-A-1 has been static: 84% of the screen unchanged."),
        "{}",
        sent.body
    );
    assert_eq!(
        sent.action_keys(),
        [
            "snooze:15",
            "snooze:60",
            "snooze:180",
            "custom",
            "blank-now"
        ]
    );
    assert_eq!(sent.action_label("snooze:60"), Some("1 h"));
    assert_eq!(sent.action_label("custom"), Some("Custom..."));
    assert_eq!(sent.urgency, Some(2));
    assert_eq!(sent.resident, Some(true));
    assert_eq!(sent.desktop_entry.as_deref(), Some(APP_ID));
    assert_eq!(sent.expire_timeout, 0);

    s.server.invoke_action(sent.id, "snooze:60").await?;
    assert_eq!(
        outcome(task).await?,
        Ok(PromptOutcome::Snooze(Duration::from_hours(1)))
    );
    // Resident notifications stay up after an action until closed.
    assert_eq!(s.server.close_calls(), [sent.id]);
    assert_eq!(s.server.open_ids(), Vec::<u32>::new());
    Ok(())
}

#[tokio::test]
async fn the_countdown_replaces_the_same_notification() -> TestResult {
    let Some(s) = Setup::with_prompter(|address| {
        NotificationPrompter::at_address(address, PromptUrgency::Critical).with_tick(TICK)
    })
    .await?
    else {
        return Ok(());
    };
    let task = s.show(request());
    let sent = s.notified(7).await?;
    let id = sent[0].id;
    let summaries: Vec<_> = sent.iter().map(|n| n.summary.as_str()).collect();
    assert_eq!(
        summaries,
        [
            "Blanking the screen in 1 min",
            "Blanking the screen in 50 s",
            "Blanking the screen in 40 s",
            "Blanking the screen in 30 s",
            "Blanking the screen in 20 s",
            "Blanking the screen in 10 s",
            "Blanking the screen now",
        ]
    );
    assert_eq!(sent[0].replaces_id, 0);
    assert!(sent[1..].iter().all(|n| n.replaces_id == id && n.id == id));
    assert!(sent.iter().all(|n| n.actions == sent[0].actions));
    assert_eq!(s.server.open_ids(), [id]);

    // The countdown stops at zero; the state machine's timer acts and
    // dismisses the prompt.
    sleep(TICK * 10).await;
    assert_eq!(s.server.notifications().len(), 7);
    s.prompter.dismiss().await?;
    assert_eq!(s.server.close_calls(), [id]);
    assert_eq!(outcome(task).await?, Ok(PromptOutcome::Dismissed));
    assert_eq!(s.server.open_ids(), Vec::<u32>::new());
    Ok(())
}

#[tokio::test]
async fn actions_map_to_outcomes() -> TestResult {
    let Some(s) = Setup::start().await? else {
        return Ok(());
    };
    let cases = [
        ("snooze:15", PromptOutcome::Snooze(Duration::from_mins(15))),
        ("snooze:180", PromptOutcome::Snooze(Duration::from_hours(3))),
        ("custom", PromptOutcome::CustomRequested),
        ("blank-now", PromptOutcome::Timeout),
    ];
    for (key, expected) in cases {
        let (task, sent) = s.shown().await?;
        s.server.invoke_action(sent.id, key).await?;
        assert_eq!(outcome(task).await?, Ok(expected), "{key}");
    }
    Ok(())
}

#[tokio::test]
async fn unknown_actions_and_other_notifications_are_ignored() -> TestResult {
    let Some(s) = Setup::start().await? else {
        return Ok(());
    };
    let (task, sent) = s.shown().await?;
    s.server.invoke_action(sent.id, "default").await?;
    s.server.invoke_action(sent.id + 100, "snooze:15").await?;
    s.server.close(sent.id + 100, CLOSED_DISMISSED).await?;
    sleep(Duration::from_millis(100)).await;
    assert!(!task.is_finished());
    s.server.invoke_action(sent.id, "custom").await?;
    assert_eq!(outcome(task).await?, Ok(PromptOutcome::CustomRequested));
    Ok(())
}

#[tokio::test]
async fn closing_without_an_action_is_dismissed() -> TestResult {
    let Some(s) = Setup::start().await? else {
        return Ok(());
    };
    let (task, sent) = s.shown().await?;
    s.server.close(sent.id, CLOSED_DISMISSED).await?;
    assert_eq!(outcome(task).await?, Ok(PromptOutcome::Dismissed));
    assert_eq!(s.server.close_calls(), Vec::<u32>::new());
    s.prompter.dismiss().await?;
    assert_eq!(s.server.close_calls(), Vec::<u32>::new());
    Ok(())
}

#[tokio::test]
async fn a_failing_notify_fails_fast() -> TestResult {
    let Some(s) = Setup::start().await? else {
        return Ok(());
    };
    s.server.fail_notify(true);
    let result = timeout(FAST, s.prompter.show(request())).await?;
    assert!(
        matches!(result, Err(BackendError::Unavailable(ref detail)) if detail.contains("fail")),
        "{result:?}"
    );
    Ok(())
}

#[tokio::test]
async fn a_failing_countdown_update_fails_the_prompt() -> TestResult {
    let Some(s) = Setup::with_prompter(|address| {
        NotificationPrompter::at_address(address, PromptUrgency::Critical).with_tick(TICK)
    })
    .await?
    else {
        return Ok(());
    };
    let (task, _) = s.shown().await?;
    s.server.fail_notify(true);
    assert!(matches!(
        outcome(task).await?,
        Err(BackendError::Unavailable(_))
    ));
    Ok(())
}

#[tokio::test]
async fn a_server_without_actions_is_unsupported() -> TestResult {
    let Some(bus) = PrivateBus::start()? else {
        return Ok(());
    };
    let server = FakeNotificationServer::with_capabilities(&bus, &["body", "persistence"]).await?;
    let prompter = NotificationPrompter::at_address(bus.address(), PromptUrgency::Critical);
    let result = timeout(FAST, prompter.show(request())).await?;
    assert!(
        matches!(result, Err(BackendError::Unsupported(_))),
        "{result:?}"
    );
    assert_eq!(server.notifications(), []);
    Ok(())
}

#[tokio::test]
async fn a_missing_server_fails_fast() -> TestResult {
    let Some(bus) = PrivateBus::start()? else {
        return Ok(());
    };
    let prompter = NotificationPrompter::at_address(bus.address(), PromptUrgency::Critical);
    let result = timeout(FAST, prompter.show(request())).await?;
    assert!(
        matches!(result, Err(BackendError::Unavailable(_))),
        "{result:?}"
    );
    let reminder = Reminder::PanelCare {
        screen_on: Duration::from_hours(4),
    };
    let result = timeout(FAST, prompter.remind(reminder)).await?;
    assert!(
        matches!(result, Err(BackendError::Unavailable(_))),
        "{result:?}"
    );
    // Nothing was ever shown, so there's nothing to close.
    timeout(FAST, prompter.dismiss()).await??;
    Ok(())
}

#[tokio::test]
async fn a_missing_bus_fails_fast() -> TestResult {
    let prompter = NotificationPrompter::at_address(
        "unix:path=/nonexistent/stillwatch/bus",
        PromptUrgency::Critical,
    );
    let result = timeout(FAST, prompter.show(request())).await?;
    assert!(
        matches!(result, Err(BackendError::Unavailable(_))),
        "{result:?}"
    );
    Ok(())
}

#[tokio::test]
async fn losing_the_bus_fails_the_prompt() -> TestResult {
    let Some(mut s) = Setup::start().await? else {
        return Ok(());
    };
    let (task, _) = s.shown().await?;
    s.bus.stop();
    assert!(outcome(task).await?.is_err());
    Ok(())
}

#[tokio::test]
async fn dropping_show_closes_the_prompt() -> TestResult {
    let Some(s) = Setup::start().await? else {
        return Ok(());
    };
    let (task, first) = s.shown().await?;
    // The countdown tick means `show` got past `Notify` and is tracking an id.
    // Aborting in the gap after the server records `Notify` drops the call
    // before that id exists, so nothing is closed.
    let sent = s.notified(2).await?;
    let current = sent.last().expect("replacement").id;
    task.abort();
    wait_until(|| s.server.close_calls().contains(&current).then_some(())).await?;
    assert_eq!(s.server.open_ids(), Vec::<u32>::new());
    s.prompter.dismiss().await?;
    assert!(s.server.close_calls().contains(&first.id));
    assert!(s.server.close_calls().contains(&current));
    Ok(())
}

#[tokio::test]
async fn a_new_prompt_closes_the_open_one() -> TestResult {
    let Some(s) = Setup::start().await? else {
        return Ok(());
    };
    let (first, old) = s.shown().await?;
    let (second, new) = s.shown().await?;
    assert_eq!(new.replaces_id, 0);
    assert_ne!(new.id, old.id);
    assert_eq!(outcome(first).await?, Ok(PromptOutcome::Dismissed));
    assert_eq!(s.server.close_calls(), [old.id]);
    assert_eq!(s.server.open_ids(), [new.id]);
    s.server.invoke_action(new.id, "snooze:15").await?;
    assert_eq!(
        outcome(second).await?,
        Ok(PromptOutcome::Snooze(Duration::from_mins(15)))
    );
    Ok(())
}

#[tokio::test]
async fn urgency_follows_the_config() -> TestResult {
    let Some(s) = Setup::start().await? else {
        return Ok(());
    };
    for (urgency, level) in [
        (PromptUrgency::Low, 0),
        (PromptUrgency::Normal, 1),
        (PromptUrgency::Critical, 2),
    ] {
        s.prompter.set_urgency(urgency);
        let (task, sent) = s.shown().await?;
        assert_eq!(sent.urgency, Some(level), "{urgency:?}");
        task.abort();
    }
    Ok(())
}

#[tokio::test]
async fn the_panel_care_reminder_is_a_plain_notification() -> TestResult {
    let Some(s) = Setup::start().await? else {
        return Ok(());
    };
    let reminder = Reminder::PanelCare {
        screen_on: Duration::from_mins(4 * 60 + 30),
    };
    timeout(FAST, s.prompter.remind(reminder)).await??;
    let sent = s.server.notifications();
    assert_eq!(sent.len(), 1);
    let note = &sent[0];
    assert_eq!(note.app_name, APP_ID);
    assert_eq!(note.summary, "Give the display a rest");
    assert!(note.body.contains("4 h 30 min"), "{}", note.body);
    assert_eq!(note.actions, Vec::<String>::new());
    assert_eq!(note.urgency, Some(1));
    assert_eq!(note.resident, None);
    assert_eq!(note.expire_timeout, -1);
    // A reminder isn't the prompt: dismissing the prompt leaves it alone.
    s.prompter.dismiss().await?;
    assert_eq!(s.server.close_calls(), Vec::<u32>::new());
    Ok(())
}
