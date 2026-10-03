use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use stillwatch_core::backend::BoxFuture;
use stillwatch_core::prompt::{PromptOutcome, PromptRequest};

use super::KdialogLauncher;
use crate::process::scripted::ScriptedRunner;
use crate::process::{CommandOutput, CommandResult, CommandRunner, CommandSpec};
use crate::prompt::dialog::DialogLauncher;

fn request() -> PromptRequest {
    PromptRequest {
        countdown: Duration::from_secs(30),
        presets: vec![Duration::from_mins(15)],
        allow_custom: false,
        stale_outputs: vec![],
    }
}

#[tokio::test]
async fn a_menu_choice_is_the_outcome() {
    let runner = Arc::new(ScriptedRunner::new());
    runner.push(Ok(CommandOutput {
        stdout: "snooze:15\n".into(),
        stderr: String::new(),
    }));
    let launcher = KdialogLauncher::new(Arc::clone(&runner) as Arc<dyn CommandRunner>);
    let outcome = launcher.launch(request()).await.unwrap();
    assert_eq!(outcome, PromptOutcome::Snooze(Duration::from_mins(15)));
    assert_eq!(runner.calls()[0].program, "kdialog");
    assert!(runner.calls()[0].args.contains(&"--menu".to_owned()));
}

#[tokio::test]
async fn dismiss_aborts_a_dialog_that_has_not_answered() {
    let hang = Arc::new(Hang::new());
    let launcher = Arc::new(KdialogLauncher::new(
        Arc::clone(&hang) as Arc<dyn CommandRunner>
    ));
    let task = {
        let launcher = Arc::clone(&launcher);
        tokio::spawn(async move { launcher.launch(request()).await })
    };
    while !hang.started.load(Ordering::SeqCst) {
        tokio::task::yield_now().await;
    }
    launcher.dismiss().await.unwrap();
    let outcome = tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(outcome, PromptOutcome::Dismissed);
}

struct Hang {
    started: AtomicBool,
}

impl Hang {
    fn new() -> Self {
        Self {
            started: AtomicBool::new(false),
        }
    }
}

impl CommandRunner for Hang {
    fn run<'a>(&'a self, _spec: &'a CommandSpec) -> BoxFuture<'a, CommandResult> {
        self.started.store(true, Ordering::SeqCst);
        Box::pin(std::future::pending())
    }
}
