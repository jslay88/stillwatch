//! Prompt answers against a fake daemon on a private bus.
//!
//! This does not start `stillwatch-gui`, talk to the session bus, or blank
//! anything. It checks that a dialog answer becomes `PromptAnswer`.

use std::sync::Arc;
use std::time::Duration;

use stillwatch_core::config::PromptConfig;
use stillwatch_core::prompt::PromptOutcome;
use stillwatch_ipc::proxy::StillwatchProxy;
use stillwatch_testkit::PrivateBus;
use stillwatchd::service::Service;
use stillwatchd::service::fake::FakeHandle;
use zbus::proxy::CacheProperties;

use stillwatch_gui::{Dialog, Input, Step, answer_prompt, update_prompt};

#[tokio::test]
async fn dialog_answers_arrive_as_prompt_answer() {
    let Some(bus) = PrivateBus::start().unwrap() else {
        eprintln!("skipping: dbus-daemon is not installed");
        return;
    };
    let server = bus.connect().await.unwrap();
    let client = bus.connect().await.unwrap();
    let fake = Arc::new(FakeHandle::new());
    let _service = Service::claim(server, Arc::clone(&fake) as _)
        .await
        .unwrap();
    let proxy = StillwatchProxy::builder(&client)
        .cache_properties(CacheProperties::No)
        .build()
        .await
        .unwrap();

    let dialog = Dialog::new(&PromptConfig::default(), 30, false);
    for (input, outcome) in [
        (
            Input::Snooze(15),
            PromptOutcome::Snooze(Duration::from_mins(15)),
        ),
        (Input::Cancel, PromptOutcome::Cancel),
        (Input::BlankNow, PromptOutcome::Timeout),
        (Input::Dismiss, PromptOutcome::Dismissed),
    ] {
        let mut one = dialog.clone();
        let Step::Answer(kind, minutes) = update_prompt(&mut one, input) else {
            panic!("expected an answer");
        };
        answer_prompt(&proxy, kind, minutes).await.unwrap();
        let encoded = stillwatch_ipc::prompt::answer_from_outcome(outcome).unwrap();
        assert_eq!(encoded, (kind, minutes));
    }

    assert_eq!(
        fake.state().answers,
        vec![
            PromptOutcome::Snooze(Duration::from_mins(15)),
            PromptOutcome::Cancel,
            PromptOutcome::Timeout,
            PromptOutcome::Dismissed,
        ]
    );
}
