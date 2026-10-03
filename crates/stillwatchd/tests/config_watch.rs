//! The config watcher and reloader against a real `notify` watcher in a temp
//! directory: every way an editor, the GUI, or a dotfiles manager can change
//! the file.

use std::error::Error;
use std::io::Write as _;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use stillwatch_core::config::ConfigError;
use stillwatch_ipc::config_file;
use stillwatchd::config_watch::{
    Applied, ConfigWatcher, ReloadOutcome, ReloadSignal, ReloadTrigger, Reloader, reload_and_report,
};
use stillwatchd::service::{ReloadReport, ServiceError};
use tempfile::TempDir;
use tokio::time::timeout;

const DEBOUNCE: Duration = Duration::from_millis(150);
const WAIT: Duration = Duration::from_secs(10);

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

/// Every `ConfigChanged` the reload path sent.
#[derive(Debug, Clone, Default)]
struct Recorder(Arc<Mutex<Vec<ReloadReport>>>);

impl Recorder {
    fn take(&self) -> Vec<ReloadReport> {
        std::mem::take(&mut self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

impl ReloadSignal for Recorder {
    fn config_changed(
        &self,
        report: &ReloadReport,
    ) -> impl Future<Output = Result<(), ServiceError>> + Send {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(report.clone());
        std::future::ready(Ok(()))
    }
}

struct Setup {
    _tmp: TempDir,
    root: PathBuf,
    watcher: ConfigWatcher,
    reloader: Reloader,
    signals: Recorder,
}

impl Setup {
    /// Watches `root/stillwatch/config.toml`, after `prepare` has laid out
    /// the temp directory.
    fn new(prepare: impl FnOnce(&Path) -> TestResult) -> TestResult<Self> {
        let tmp = tempfile::tempdir()?;
        let root = tmp.path().to_path_buf();
        prepare(&root)?;
        let path = root.join("stillwatch/config.toml");
        let (reloader, _) = Reloader::load(path.clone())?;
        let watcher = ConfigWatcher::with_debounce(&path, DEBOUNCE)?;
        Ok(Self {
            _tmp: tmp,
            root,
            watcher,
            reloader,
            signals: Recorder::default(),
        })
    }

    /// A config file with `stale_percent = percent`.
    fn with_file(percent: u32) -> TestResult<Self> {
        Self::new(|root| {
            std::fs::create_dir(root.join("stillwatch"))?;
            std::fs::write(root.join("stillwatch/config.toml"), config(percent))?;
            Ok(())
        })
    }

    fn path(&self) -> PathBuf {
        self.root.join("stillwatch/config.toml")
    }

    /// Waits for the watcher's next trigger and runs the reload path.
    async fn change(&mut self) -> TestResult<ReloadOutcome> {
        let trigger = timeout(WAIT, self.watcher.next())
            .await?
            .ok_or("the watcher stopped")?;
        assert_eq!(trigger, ReloadTrigger::FileChanged);
        Ok(reload_and_report(&mut self.reloader, trigger, &self.signals).await)
    }

    /// Waits until a trigger applies a config, skipping no-op triggers
    /// (a new directory before its file, for example).
    async fn applied(&mut self) -> TestResult<Applied> {
        loop {
            match self.change().await? {
                ReloadOutcome::Unchanged => {}
                ReloadOutcome::Applied(applied) => return Ok(*applied),
                ReloadOutcome::Rejected(error) => return Err(error.into()),
            }
        }
    }

    /// Asserts nothing triggers for a while.
    async fn quiet(&mut self) {
        let next = timeout(DEBOUNCE * 4, self.watcher.next()).await;
        assert!(next.is_err(), "unexpected trigger: {next:?}");
    }

    fn percent(&self) -> u32 {
        self.reloader.config().stale.stale_percent
    }
}

fn config(percent: u32) -> String {
    format!("[stale]\nstale_percent = {percent}\n")
}

fn rejected(outcome: ReloadOutcome) -> TestResult<ConfigError> {
    match outcome {
        ReloadOutcome::Rejected(error) => Ok(error),
        other => Err(format!("expected rejected, got {other:?}").into()),
    }
}

fn changed_keys(applied: &Applied) -> Vec<&'static str> {
    applied.changes.keys().collect()
}

#[tokio::test]
async fn a_write_that_renames_over_the_file() -> TestResult {
    let mut s = Setup::with_file(50)?;
    config_file::write(&s.path(), &config(60), true)?;
    let applied = s.applied().await?;
    assert_eq!(changed_keys(&applied), ["stale.stale_percent"]);
    assert_eq!(s.percent(), 60);
    assert_eq!(s.signals.take(), [ReloadReport::applied()]);
    s.quiet().await;
    Ok(())
}

#[tokio::test]
async fn an_in_place_write() -> TestResult {
    let mut s = Setup::with_file(50)?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(s.path())?;
    file.write_all(config(60).as_bytes())?;
    drop(file);
    s.applied().await?;
    assert_eq!(s.percent(), 60);
    assert_eq!(s.signals.take(), [ReloadReport::applied()]);
    Ok(())
}

#[tokio::test]
async fn a_symlinked_config_follows_its_target_and_retargets() -> TestResult {
    let mut s = Setup::new(|root| {
        for dir in ["stillwatch", "dotfiles-a", "dotfiles-b"] {
            std::fs::create_dir(root.join(dir))?;
        }
        std::fs::write(root.join("dotfiles-a/config.toml"), config(50))?;
        std::fs::write(root.join("dotfiles-b/config.toml"), config(70))?;
        symlink(
            "../dotfiles-a/config.toml",
            root.join("stillwatch/config.toml"),
        )?;
        Ok(())
    })?;
    assert_eq!(s.percent(), 50);

    let a = s.root.join("dotfiles-a/config.toml");
    config_file::write(&a, &config(60), true)?;
    s.applied().await?;
    assert_eq!(s.percent(), 60);
    assert!(s.path().is_symlink(), "the write replaced the link");

    let staged = s.root.join("stillwatch/.config.toml.link");
    symlink(s.root.join("dotfiles-b/config.toml"), &staged)?;
    std::fs::rename(&staged, s.path())?;
    s.applied().await?;
    assert_eq!(s.percent(), 70);

    config_file::write(&a, &config(80), true)?;
    s.quiet().await;

    config_file::write(&s.root.join("dotfiles-b/config.toml"), &config(90), true)?;
    s.applied().await?;
    assert_eq!(s.percent(), 90);
    assert_eq!(s.signals.take().len(), 3);
    Ok(())
}

#[tokio::test]
async fn deleting_and_recreating_the_file() -> TestResult {
    let mut s = Setup::with_file(50)?;
    std::fs::remove_file(s.path())?;
    let error = rejected(s.change().await?)?;
    assert!(matches!(error, ConfigError::NotFound { .. }), "{error:?}");
    assert_eq!(s.percent(), 50);
    let report = s.signals.take();
    assert_eq!(report.len(), 1);
    assert!(!report[0].ok);
    assert!(report[0].errors[0].starts_with("config file not found: "));

    std::fs::write(s.path(), config(60))?;
    s.applied().await?;
    assert_eq!(s.percent(), 60);
    assert_eq!(s.reloader.errors(), [] as [String; 0]);
    Ok(())
}

#[tokio::test]
async fn an_invalid_edit_keeps_the_last_good_config() -> TestResult {
    let mut s = Setup::with_file(50)?;
    config_file::write(&s.path(), &config(0), true)?;
    let error = rejected(s.change().await?)?;
    assert!(matches!(error, ConfigError::Invalid(_)), "{error:?}");
    assert_eq!(s.percent(), 50);
    let reports = s.signals.take();
    assert_eq!(reports.len(), 1);
    assert!(!reports[0].ok);
    assert!(reports[0].errors[0].starts_with("stale.stale_percent: "));
    assert_eq!(s.reloader.errors(), reports[0].errors);

    config_file::write(&s.path(), &config(60), true)?;
    s.applied().await?;
    assert_eq!(s.percent(), 60);
    assert_eq!(s.reloader.errors(), [] as [String; 0]);
    Ok(())
}

#[tokio::test]
async fn a_no_op_save_doesnt_reload() -> TestResult {
    let mut s = Setup::with_file(50)?;
    config_file::write(&s.path(), &config(50), true)?;
    assert!(matches!(s.change().await?, ReloadOutcome::Unchanged));
    assert_eq!(s.signals.take(), []);
    Ok(())
}

#[tokio::test]
async fn rapid_writes_debounce_to_one_reload() -> TestResult {
    let mut s = Setup::with_file(50)?;
    for percent in 51..=55 {
        config_file::write(&s.path(), &config(percent), true)?;
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    s.applied().await?;
    assert_eq!(s.percent(), 55);
    s.quiet().await;
    assert_eq!(s.signals.take(), [ReloadReport::applied()]);
    Ok(())
}

#[tokio::test]
async fn a_missing_directory_is_watched_for_until_it_appears() -> TestResult {
    let mut s = Setup::new(|_| Ok(()))?;
    assert_eq!(s.percent(), 70);
    std::fs::create_dir(s.root.join("stillwatch"))?;
    std::fs::write(s.path(), config(60))?;
    s.applied().await?;
    assert_eq!(s.percent(), 60);

    config_file::write(&s.path(), &config(65), true)?;
    s.applied().await?;
    assert_eq!(s.percent(), 65);
    Ok(())
}

#[tokio::test]
async fn a_removed_directory_is_picked_up_again() -> TestResult {
    let mut s = Setup::with_file(50)?;
    std::fs::remove_dir_all(s.root.join("stillwatch"))?;
    rejected(s.change().await?)?;
    s.signals.take();

    std::fs::create_dir(s.root.join("stillwatch"))?;
    std::fs::write(s.path(), config(60))?;
    s.applied().await?;
    assert_eq!(s.percent(), 60);
    Ok(())
}

#[tokio::test]
async fn unrelated_files_dont_trigger() -> TestResult {
    let mut s = Setup::with_file(50)?;
    std::fs::write(s.root.join("stillwatch/notes.txt"), "hi")?;
    std::fs::write(s.root.join("elsewhere.toml"), "hi")?;
    s.quiet().await;
    Ok(())
}
