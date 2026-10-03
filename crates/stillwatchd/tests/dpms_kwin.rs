//! `DpmsBlanker` against a private headless `KWin` from
//! [`stillwatch_testkit::kwin`]. Both the watch and kscreen-doctor talk to
//! that compositor, never the desktop.

use std::sync::Arc;
use std::time::Duration;

use stillwatch_core::backend::{BackendError, Blanker, EventSink};
use stillwatch_core::event::Event;
use stillwatch_testkit::kwin::{Kwin, KwinOptions};
use stillwatchd::action::dpms::DpmsBlanker;
use stillwatchd::process::{CommandResult, CommandRunner, CommandSpec, TokioRunner};
use tokio::sync::mpsc;
use tokio::time::timeout;

const OUTPUTS: [&str; 2] = ["Virtual-0", "Virtual-1"];
const LIMIT: Duration = Duration::from_secs(15);

/// Runs kscreen-doctor with the harness environment so it finds the private
/// socket and bus, not the desktop's.
struct HarnessRunner {
    extra: Vec<(String, String)>,
}

impl CommandRunner for HarnessRunner {
    fn run<'a>(
        &'a self,
        spec: &'a CommandSpec,
    ) -> stillwatch_core::backend::BoxFuture<'a, CommandResult> {
        Box::pin(async move {
            let mut spec = spec.clone();
            spec.env = self.extra.iter().cloned().chain(spec.env).collect();
            TokioRunner.run(&spec).await
        })
    }
}

fn on_path(tool: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(tool).is_file()))
}

async fn start() -> Result<Option<Kwin>, stillwatch_testkit::Error> {
    if !on_path("kscreen-doctor") {
        assert!(
            std::env::var_os("STILLWATCH_REQUIRE_KWIN").is_none_or(|v| v != "1"),
            "STILLWATCH_REQUIRE_KWIN=1 but kscreen-doctor is not on PATH"
        );
        eprintln!("skipping: kscreen-doctor not found");
        return Ok(None);
    }
    Kwin::start(KwinOptions {
        outputs: 2,
        width: 1280,
        height: 720,
        ..KwinOptions::default()
    })
    .await
}

fn blanker(kwin: &Kwin) -> DpmsBlanker {
    let extra = kwin
        .env()
        .into_iter()
        .filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?)))
        .collect();
    let connect = kwin.connector();
    DpmsBlanker::new()
        .on_display(kwin.socket_path().display().to_string())
        .with_connector(move || {
            connect().map_err(|err| BackendError::Disconnected(err.to_string()))
        })
        .with_runner(Arc::new(HarnessRunner { extra }))
}

fn channel_sink() -> (Arc<dyn EventSink>, mpsc::UnboundedReceiver<Event>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let sink = move |event| {
        let _ = tx.send(event);
    };
    (Arc::new(sink), rx)
}

fn power(output: &str, on: bool) -> Event {
    Event::DisplayPower {
        output: output.to_owned(),
        on,
    }
}

/// Collects events until every output has reported `on`, sorted by output.
async fn states(rx: &mut mpsc::UnboundedReceiver<Event>, on: bool) -> Vec<Event> {
    let mut seen = Vec::new();
    let all_reported = |seen: &Vec<Event>| OUTPUTS.iter().all(|o| seen.contains(&power(o, on)));
    let _ = timeout(LIMIT, async {
        while !all_reported(&seen) {
            let Some(event) = rx.recv().await else {
                break;
            };
            seen.push(event);
        }
    })
    .await;
    assert!(
        all_reported(&seen),
        "expected every output on={on}, saw {seen:?}"
    );
    seen.sort_by_key(|event| format!("{event:?}"));
    seen
}

#[tokio::test]
async fn dpms_off_and_on_is_reported_per_output() {
    let Some(kwin) = start().await.unwrap() else {
        return;
    };
    let blanker = Arc::new(blanker(&kwin));
    let (sink, mut rx) = channel_sink();
    let watching = {
        let blanker = Arc::clone(&blanker);
        tokio::spawn(async move { blanker.watch(sink).await })
    };
    assert_eq!(states(&mut rx, true).await, OUTPUTS.map(|o| power(o, true)));

    // KWin 6.7.5 applies DPMS to the whole workspace, so the excluded output
    // goes dark too. If this starts failing because Virtual-1 stays on, KWin
    // honors --dpms-excluded now: update Platform facts and the dpms docs.
    blanker.blank(&[OUTPUTS[0].to_owned()]).await.unwrap();
    assert_eq!(
        states(&mut rx, false).await,
        OUTPUTS.map(|o| power(o, false))
    );

    blanker.unblank(&[]).await.unwrap();
    assert_eq!(states(&mut rx, true).await, OUTPUTS.map(|o| power(o, true)));
    assert!(!watching.is_finished());
    watching.abort();
}

#[tokio::test]
async fn blanking_an_unknown_output_changes_nothing() {
    let Some(kwin) = start().await.unwrap() else {
        return;
    };
    let blanker = blanker(&kwin);
    let error = blanker.blank(&["HDMI-A-9".to_owned()]).await.unwrap_err();
    assert!(error.to_string().contains("HDMI-A-9"), "{error}");
    blanker.unblank(&["HDMI-A-9".to_owned()]).await.unwrap();
}
