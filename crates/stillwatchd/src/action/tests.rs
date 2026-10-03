//! [`ActionRunner`] tests. The machine already covers re-blank and gamepad wake;
//! these check that Commands become backend calls.

use std::sync::Arc;
use std::time::Duration;

use stillwatch_core::backend::BackendError;
use stillwatch_core::command::{BlankMethod, Command, HookKind};
use stillwatch_core::config::{ActionMode, ActionOutputs, Config, DimMethod};
use stillwatch_core::event::Event;
use stillwatch_core::history::HistoryKind;
use stillwatch_core::mocks::{BlankerCall, MemoryHistory, MockBlanker, MockSessionMonitor};
use stillwatch_core::time::FakeClock;

use super::{ActionBackends, ActionRunner, ENV_METHOD, ENV_OUTPUTS, ENV_REASON, HOOK_TIMEOUT};
use crate::process::CommandError;
use crate::process::scripted::ScriptedRunner;

struct Fixture {
    runner: ActionRunner,
    dpms: Arc<MockBlanker>,
    overlay: Arc<MockBlanker>,
    ddc: Arc<MockBlanker>,
    session: Arc<MockSessionMonitor>,
    commands: Arc<ScriptedRunner>,
    history: Arc<MemoryHistory>,
}

impl Fixture {
    fn new(edit: impl FnOnce(&mut Config)) -> Self {
        let mut config = Config::default();
        edit(&mut config);
        Self::with(config, vec!["HDMI-A-1".into(), "DP-1".into()])
    }

    fn with(config: Config, connected: Vec<String>) -> Self {
        let overlay = Arc::new(MockBlanker::new());
        let dpms = Arc::new(MockBlanker::new());
        let ddc = Arc::new(MockBlanker::new());
        let session = Arc::new(MockSessionMonitor::new());
        let commands = Arc::new(ScriptedRunner::new());
        let history = Arc::new(MemoryHistory::new());
        let backends = ActionBackends {
            dpms: dpms.clone(),
            overlay: overlay.clone(),
            ddc: ddc.clone(),
            dimmer: overlay.clone(),
            session: session.clone(),
            commands: commands.clone(),
            history: history.clone(),
            clock: Arc::new(FakeClock::new()),
            brightness: None,
        };
        Self {
            runner: ActionRunner::new(config, connected, backends),
            dpms,
            overlay,
            ddc,
            session,
            commands,
            history,
        }
    }

    async fn blank(&self, method: BlankMethod) -> Event {
        self.runner
            .execute(&Command::Blank {
                outputs: vec![],
                method,
            })
            .await
            .expect("blank replies")
    }

    async fn blank_named(&self, outputs: Vec<String>, method: BlankMethod) -> Event {
        self.runner
            .execute(&Command::Blank { outputs, method })
            .await
            .expect("blank replies")
    }

    async fn unblank(&self) {
        assert_eq!(
            self.runner
                .execute(&Command::Unblank { outputs: vec![] })
                .await,
            None
        );
    }

    async fn flush_hooks(&self) {
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;
    }
}

#[tokio::test]
async fn blank_picks_the_method() {
    let f = Fixture::new(|_| {});
    assert_eq!(f.blank(BlankMethod::Dpms).await, Event::ActionCompleted);
    assert_eq!(f.dpms.calls(), [BlankerCall::Blank(f.runner.targets())]);
    assert_eq!(f.overlay.calls(), []);
    assert_eq!(f.ddc.calls(), []);

    let f = Fixture::new(|_| {});
    assert_eq!(f.blank(BlankMethod::Overlay).await, Event::ActionCompleted);
    assert_eq!(f.overlay.calls(), [BlankerCall::Blank(f.runner.targets())]);

    let f = Fixture::new(|_| {});
    assert_eq!(
        f.blank(BlankMethod::DdcStandby).await,
        Event::ActionCompleted
    );
    assert_eq!(f.ddc.calls(), [BlankerCall::Blank(f.runner.targets())]);
}

#[tokio::test]
async fn lock_skips_when_already_locked() {
    let f = Fixture::new(|c| c.action.mode = ActionMode::LockAndBlank);
    f.session.set_locked(true);
    assert_eq!(
        f.runner.execute(&Command::Lock).await,
        Some(Event::ActionCompleted)
    );
    assert_eq!(f.session.lock_count(), 0);

    f.session.set_locked(false);
    assert_eq!(
        f.runner.execute(&Command::Lock).await,
        Some(Event::ActionCompleted)
    );
    assert_eq!(f.session.lock_count(), 1);
    assert_eq!(f.blank(BlankMethod::Dpms).await, Event::ActionCompleted);
}

#[tokio::test]
async fn command_mode_runs_sh_c_and_does_not_blank() {
    let f = Fixture::new(|c| {
        c.action.mode = ActionMode::Command;
        c.action.command = "my-screen-off".into();
    });
    assert_eq!(
        f.runner
            .execute(&Command::RunHook(HookKind::ActionCommand))
            .await,
        None
    );
    f.flush_hooks().await;
    let calls = f.commands.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].program, "sh");
    assert_eq!(calls[0].args, ["-c", "my-screen-off"]);
    assert_eq!(
        calls[0]
            .env
            .iter()
            .find(|(k, _)| k == ENV_REASON)
            .map(|(_, v)| v.as_str()),
        Some("command")
    );
    assert_eq!(f.dpms.calls(), []);
}

#[tokio::test(start_paused = true)]
async fn dim_then_blank_dims_waits_then_blanks() {
    let f = Fixture::new(|c| {
        c.action.mode = ActionMode::DimThenBlank;
        c.action.dim_percent = 20;
        c.action.dim_seconds = 30;
        c.action.blank_method = BlankMethod::Dpms;
    });
    let task = {
        let runner = f.runner.clone();
        tokio::spawn(async move {
            runner
                .execute(&Command::Blank {
                    outputs: vec![],
                    method: BlankMethod::Dpms,
                })
                .await
        })
    };
    tokio::task::yield_now().await;
    assert_eq!(
        f.overlay.calls(),
        [BlankerCall::Dim {
            outputs: f.runner.targets(),
            percent: 20,
        }]
    );
    assert_eq!(f.dpms.calls(), []);
    tokio::time::advance(Duration::from_secs(30)).await;
    assert_eq!(task.await.expect("join"), Some(Event::ActionCompleted));
    let targets = f.runner.targets();
    assert_eq!(
        f.overlay.calls(),
        [
            BlankerCall::Dim {
                outputs: targets.clone(),
                percent: 20,
            },
            BlankerCall::Undim(targets.clone()),
        ]
    );
    assert_eq!(f.dpms.calls(), [BlankerCall::Blank(targets)]);
}

#[tokio::test(start_paused = true)]
async fn dim_then_overlay_blank_leaves_the_dim_on_the_surface() {
    let f = Fixture::new(|c| {
        c.action.mode = ActionMode::DimThenBlank;
        c.action.dim_seconds = 1;
        c.action.blank_method = BlankMethod::Overlay;
    });
    let task = {
        let runner = f.runner.clone();
        tokio::spawn(async move {
            runner
                .execute(&Command::Blank {
                    outputs: vec![],
                    method: BlankMethod::Overlay,
                })
                .await
        })
    };
    tokio::time::advance(Duration::from_secs(1)).await;
    assert_eq!(task.await.expect("join"), Some(Event::ActionCompleted));
    let targets = f.runner.targets();
    assert_eq!(
        f.overlay.calls(),
        [
            BlankerCall::Dim {
                outputs: targets.clone(),
                percent: 20,
            },
            BlankerCall::Blank(targets),
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn dim_cancelled_by_unblank() {
    let f = Fixture::new(|c| {
        c.action.mode = ActionMode::DimThenBlank;
        c.action.dim_seconds = 30;
    });
    let task = {
        let runner = f.runner.clone();
        tokio::spawn(async move {
            runner
                .execute(&Command::Blank {
                    outputs: vec![],
                    method: BlankMethod::Dpms,
                })
                .await
        })
    };
    tokio::task::yield_now().await;
    f.unblank().await;
    let result = task.await.expect("join");
    assert!(
        matches!(result, Some(Event::ActionFailed { error: BackendError::Io(ref msg) }) if msg.contains("Unblank")),
        "{result:?}"
    );
    let targets = f.runner.targets();
    assert!(
        !f.dpms
            .calls()
            .iter()
            .any(|call| matches!(call, BlankerCall::Blank(_))),
        "cancelled dim must not blank: {:?}",
        f.dpms.calls()
    );
    assert!(
        f.overlay
            .calls()
            .iter()
            .any(|call| matches!(call, BlankerCall::Undim(out) if *out == targets))
    );
}

#[tokio::test]
async fn reblank_skips_dim() {
    let f = Fixture::new(|c| {
        c.action.mode = ActionMode::DimThenBlank;
        c.action.dim_seconds = 0;
    });
    assert_eq!(f.blank(BlankMethod::Dpms).await, Event::ActionCompleted);
    let after_first = f.overlay.calls().len();
    assert_eq!(f.blank(BlankMethod::Overlay).await, Event::ActionCompleted);
    let extra = &f.overlay.calls()[after_first..];
    assert!(
        !extra
            .iter()
            .any(|call| matches!(call, BlankerCall::Dim { .. })),
        "re-blank must not dim: {extra:?}"
    );
    assert!(extra.contains(&BlankerCall::Blank(f.runner.targets())));
}

#[tokio::test]
async fn brightness_dim_falls_back_to_overlay() {
    let f = Fixture::new(|c| {
        c.action.mode = ActionMode::DimThenBlank;
        c.action.dim_method = DimMethod::Brightness;
        c.action.dim_seconds = 0;
    });
    assert_eq!(f.blank(BlankMethod::Dpms).await, Event::ActionCompleted);
    assert!(
        f.overlay
            .calls()
            .iter()
            .any(|call| matches!(call, BlankerCall::Dim { percent: 20, .. }))
    );
}

#[tokio::test]
async fn hooks_carry_env_and_do_not_block() {
    let f = Fixture::new(|c| {
        c.action.on_blank_cmd = "tv-off".into();
        c.action.on_resume_cmd = "tv-on".into();
    });
    assert_eq!(
        f.blank(BlankMethod::DdcStandby).await,
        Event::ActionCompleted
    );
    assert_eq!(
        f.runner.execute(&Command::RunHook(HookKind::OnBlank)).await,
        None
    );
    f.unblank().await;
    assert_eq!(
        f.runner
            .execute(&Command::RunHook(HookKind::OnResume))
            .await,
        None
    );
    f.flush_hooks().await;
    let calls = f.commands.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].args, ["-c", "tv-off"]);
    assert_eq!(calls[1].args, ["-c", "tv-on"]);
    for (call, reason) in [(&calls[0], "blank"), (&calls[1], "resume")] {
        assert_eq!(call.timeout, HOOK_TIMEOUT);
        let env: Vec<_> = call
            .env
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        assert!(env.contains(&(ENV_OUTPUTS, "HDMI-A-1,DP-1")));
        assert!(env.contains(&(ENV_METHOD, "ddc_standby")));
        assert!(env.contains(&(ENV_REASON, reason)));
    }
}

#[tokio::test]
async fn panel_care_trigger_uses_the_same_hook_runner() {
    let f = Fixture::new(|c| c.panel_care.trigger_cmd = "pixel-clean".into());
    assert_eq!(
        f.runner
            .execute(&Command::RunHook(HookKind::PanelCareTrigger))
            .await,
        None
    );
    f.flush_hooks().await;
    let calls = f.commands.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].args, ["-c", "pixel-clean"]);
    let env: Vec<_> = calls[0]
        .env
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    assert!(env.contains(&(ENV_REASON, "panel_care")));
}

#[tokio::test]
async fn hook_timeout_and_failure_are_logged_not_returned() {
    let f = Fixture::new(|c| c.action.on_blank_cmd = "hang".into());
    f.commands.push_error(CommandError::TimedOut {
        program: "sh".into(),
        after: HOOK_TIMEOUT,
    });
    assert_eq!(
        f.runner.execute(&Command::RunHook(HookKind::OnBlank)).await,
        None
    );
    f.flush_hooks().await;
    assert_eq!(f.commands.calls().len(), 1);
}

#[tokio::test]
async fn blanker_failure_falls_back_to_overlay_and_records_it() {
    let f = Fixture::new(|_| {});
    f.dpms
        .fail_next(BackendError::Unavailable("kscreen-doctor".into()));
    assert_eq!(f.blank(BlankMethod::Dpms).await, Event::ActionCompleted);
    assert_eq!(f.overlay.calls(), [BlankerCall::Blank(f.runner.targets())]);
    let kinds: Vec<_> = f.history.entries().iter().map(|e| e.kind).collect();
    assert_eq!(kinds, [HistoryKind::OverlayUsed]);
    assert_eq!(
        f.history.entries()[0].blank_method,
        Some(BlankMethod::Overlay)
    );
}

#[tokio::test]
async fn overlay_failure_does_not_fallback_again() {
    let f = Fixture::new(|_| {});
    f.overlay
        .fail_next(BackendError::Unsupported("no layer shell".into()));
    let event = f.blank(BlankMethod::Overlay).await;
    assert!(matches!(
        event,
        Event::ActionFailed {
            error: BackendError::Unsupported(_)
        }
    ));
    assert_eq!(f.history.entries(), []);
}

#[tokio::test]
async fn fallback_failure_is_action_failed() {
    let f = Fixture::new(|_| {});
    f.dpms.fail_next(BackendError::NotFound("HDMI-A-1".into()));
    f.overlay
        .fail_next(BackendError::Disconnected("gone".into()));
    assert!(matches!(
        f.blank(BlankMethod::Dpms).await,
        Event::ActionFailed {
            error: BackendError::Disconnected(_)
        }
    ));
}

#[tokio::test]
async fn outputs_list_is_respected() {
    let f = Fixture::new(|c| {
        c.action.outputs = ActionOutputs::Monitored;
        c.stale.monitored_outputs = vec!["HDMI-A-1".into()];
    });
    assert_eq!(f.runner.targets(), ["HDMI-A-1"]);
    assert_eq!(
        f.blank_named(vec!["HDMI-A-1".into()], BlankMethod::Dpms)
            .await,
        Event::ActionCompleted
    );
    assert_eq!(
        f.dpms.calls(),
        [BlankerCall::Blank(vec!["HDMI-A-1".into()])]
    );

    let f = Fixture::new(|c| {
        c.action.outputs = ActionOutputs::All;
        c.stale.monitored_outputs = vec!["HDMI-A-1".into()];
    });
    assert_eq!(f.runner.targets(), ["HDMI-A-1", "DP-1"]);
    assert_eq!(
        f.blank(BlankMethod::DdcStandby).await,
        Event::ActionCompleted
    );
    assert_eq!(f.ddc.calls(), [BlankerCall::Blank(f.runner.targets())]);

    let f = Fixture::with(Config::default(), vec!["HDMI-A-1".into()]);
    assert_eq!(f.runner.targets(), ["HDMI-A-1"]);
}

#[tokio::test]
async fn unblank_wakes_every_blanker() {
    let f = Fixture::new(|_| {});
    assert_eq!(f.blank(BlankMethod::Dpms).await, Event::ActionCompleted);
    f.unblank().await;
    let targets = f.runner.targets();
    assert!(
        f.dpms
            .calls()
            .contains(&BlankerCall::Unblank(targets.clone()))
    );
    assert!(
        f.overlay
            .calls()
            .contains(&BlankerCall::Unblank(targets.clone()))
    );
    assert!(f.ddc.calls().contains(&BlankerCall::Unblank(targets)));
}

#[tokio::test]
async fn apply_config_and_connected_update_targets() {
    let f = Fixture::new(|_| {});
    f.runner.set_connected(vec!["eDP-1".into()]);
    let mut config = Config::default();
    config.action.outputs = ActionOutputs::Monitored;
    config.stale.monitored_outputs = vec!["eDP-1".into()];
    f.runner.apply_config(&config);
    assert_eq!(f.runner.targets(), ["eDP-1"]);
}

#[tokio::test]
async fn other_commands_are_ignored() {
    assert_eq!(
        Fixture::new(|_| {})
            .runner
            .execute(&Command::DismissPrompt)
            .await,
        None
    );
}
