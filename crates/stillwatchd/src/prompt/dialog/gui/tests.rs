use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use stillwatch_core::backend::{BackendError, BoxFuture};
use stillwatch_core::prompt::{PromptOutcome, PromptRequest};

use super::{GuiLauncher, command_spec};
use crate::process::scripted::ScriptedRunner;
use crate::process::{CommandError, CommandOutput, CommandResult, CommandRunner, CommandSpec};
use crate::prompt::dialog::DialogLauncher;

fn request(seconds: u64) -> PromptRequest {
    PromptRequest {
        countdown: Duration::from_secs(seconds),
        presets: vec![Duration::from_mins(15)],
        allow_custom: true,
        stale_outputs: vec![],
    }
}

fn program_is_gui(program: &str) -> bool {
    program == "stillwatch-gui" || program.ends_with("/stillwatch-gui")
}

#[test]
fn the_command_is_prompt_with_the_remaining_seconds() {
    let spec = command_spec(&request(45), false);
    assert!(program_is_gui(&spec.program), "{}", spec.program);
    assert_eq!(spec.timeout, Duration::from_secs(75));
    assert_eq!(spec.args, ["prompt", "--remaining", "45"]);
    let custom = command_spec(&request(12), true);
    assert_eq!(custom.args, ["prompt", "--remaining", "12", "--custom"]);
}

#[tokio::test]
async fn exit_zero_does_not_resolve_with_an_outcome() {
    let runner = Arc::new(ScriptedRunner::new());
    runner.push(Ok(CommandOutput::default()));
    let launcher = GuiLauncher::new(Arc::clone(&runner) as Arc<dyn CommandRunner>);
    let result =
        tokio::time::timeout(Duration::from_millis(50), launcher.launch(request(30))).await;
    assert!(result.is_err(), "exit 0 must not become a second answer");
    assert!(program_is_gui(&runner.calls()[0].program));
    assert_eq!(runner.calls()[0].args, ["prompt", "--remaining", "30"]);
}

#[tokio::test]
async fn exit_one_is_dismissed_and_a_missing_gui_is_an_error() {
    let runner = Arc::new(ScriptedRunner::new());
    runner.push(Err(CommandError::Failed {
        program: "stillwatch-gui".into(),
        code: Some(1),
        stderr: String::new(),
    }));
    let launcher = GuiLauncher::new(Arc::clone(&runner) as Arc<dyn CommandRunner>);
    assert_eq!(
        launcher.launch(request(10)).await.unwrap(),
        PromptOutcome::Dismissed
    );

    runner.push(Err(CommandError::NotFound {
        program: "stillwatch-gui".into(),
    }));
    assert!(matches!(
        launcher.launch_custom(request(10)).await,
        Err(BackendError::Unavailable(_))
    ));
    assert!(runner.calls()[1].args.contains(&"--custom".to_owned()));
}

#[tokio::test]
async fn dismiss_aborts_a_dialog_that_has_not_exited() {
    let hang = Arc::new(Hang::new());
    let launcher = Arc::new(GuiLauncher::new(Arc::clone(&hang) as Arc<dyn CommandRunner>));
    let task = {
        let launcher = Arc::clone(&launcher);
        tokio::spawn(async move { launcher.launch(request(30)).await })
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
