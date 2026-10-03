//! Watching the config directory with `notify` and debouncing what it sees.
//!
//! Editors and the GUI's atomic write replace `config.toml` by renaming over
//! it, so the directory is watched, never the file. A symlinked config also
//! watches the directory of every link target, and the watches move when a
//! link changes. A directory that doesn't exist yet is covered by watching
//! its closest existing ancestor until it appears.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use notify::event::{AccessKind, AccessMode};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::{Instant, sleep_until};

use super::ReloadTrigger;
use super::targets::Targets;

/// How long the directory has to be quiet after an event before a reload
/// is triggered, so one save (several events) reloads once.
pub const DEFAULT_DEBOUNCE: Duration = Duration::from_millis(300);

/// Why the watcher couldn't start.
#[derive(Debug, thiserror::Error)]
pub enum WatchError {
    /// The platform watcher (inotify) couldn't be created.
    #[error("can't start the config file watcher: {0}")]
    Start(#[source] notify::Error),
    /// A directory couldn't be watched, for example because the inotify
    /// watch limit is reached.
    #[error("can't watch {}: {source}", path.display())]
    Watch {
        /// The directory.
        path: PathBuf,
        /// What `notify` reported.
        source: notify::Error,
    },
}

/// Watches the config file's directories and yields
/// [`ReloadTrigger::FileChanged`] once things settle after a change.
///
/// Triggers coalesce: if the previous one hasn't been taken yet, a new
/// change doesn't queue another. Dropping the watcher stops its task and
/// the inotify watches.
#[derive(Debug)]
pub struct ConfigWatcher {
    triggers: mpsc::Receiver<ReloadTrigger>,
    task: JoinHandle<()>,
}

impl ConfigWatcher {
    /// Starts watching for `path` with [`DEFAULT_DEBOUNCE`]. Must be called
    /// inside a tokio runtime.
    ///
    /// # Errors
    ///
    /// Fails if the watcher can't be created or a directory can't be watched.
    pub fn spawn(path: &Path) -> Result<Self, WatchError> {
        Self::with_debounce(path, DEFAULT_DEBOUNCE)
    }

    /// Starts watching for `path`, waiting `debounce` of quiet after the last
    /// event before triggering.
    ///
    /// # Errors
    ///
    /// Fails if the watcher can't be created or a directory can't be watched.
    pub fn with_debounce(path: &Path, debounce: Duration) -> Result<Self, WatchError> {
        let (events_tx, events) = mpsc::unbounded_channel();
        let watcher = notify::recommended_watcher(move |event| {
            let _ = events_tx.send(event);
        })
        .map_err(WatchError::Start)?;
        let mut tracked = Watches::new(path, watcher);
        for dir in tracked.targets.dirs().clone() {
            tracked.add(&dir).map_err(|source| WatchError::Watch {
                path: dir.clone(),
                source,
            })?;
        }
        tracing::debug!(dirs = ?tracked.watched, "watching for config changes");
        let (triggers_tx, triggers) = mpsc::channel(1);
        let task = tokio::spawn(run(tracked, events, triggers_tx, debounce));
        Ok(Self { triggers, task })
    }

    /// Waits for the next change. `None` if the watcher stopped.
    pub async fn next(&mut self) -> Option<ReloadTrigger> {
        self.triggers.recv().await
    }
}

impl Drop for ConfigWatcher {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// The `notify` watcher and the directories it currently watches.
struct Watches {
    path: PathBuf,
    watcher: RecommendedWatcher,
    targets: Targets,
    watched: BTreeSet<PathBuf>,
}

impl Watches {
    fn new(path: &Path, watcher: RecommendedWatcher) -> Self {
        Self {
            path: path.to_path_buf(),
            watcher,
            targets: Targets::resolve(path),
            watched: BTreeSet::new(),
        }
    }

    fn add(&mut self, dir: &Path) -> notify::Result<()> {
        self.watcher.watch(dir, RecursiveMode::NonRecursive)?;
        self.watched.insert(dir.to_path_buf());
        Ok(())
    }

    /// Whether `event` can affect the config. Relevant events also move the
    /// watches to wherever the config (and its links) now live.
    fn handle(&mut self, event: &Event) -> bool {
        let relevant = event.need_rescan()
            || (!is_read(event.kind)
                && event
                    .paths
                    .iter()
                    .any(|path| self.targets.is_relevant(path)));
        if relevant {
            for path in &event.paths {
                if self.watched.contains(path) && !path.is_dir() {
                    self.watched.remove(path);
                }
            }
            self.retarget();
        }
        relevant
    }

    fn retarget(&mut self) {
        self.targets = Targets::resolve(&self.path);
        let wanted = self.targets.dirs().clone();
        for dir in self.watched.difference(&wanted) {
            let _ = self.watcher.unwatch(dir);
        }
        self.watched.retain(|dir| wanted.contains(dir));
        for dir in wanted.difference(&self.watched.clone()) {
            match self.add(dir) {
                Ok(()) => tracing::debug!(dir = %dir.display(), "watching for config changes"),
                Err(error) => tracing::warn!(
                    dir = %dir.display(),
                    %error,
                    "can't watch for config changes"
                ),
            }
        }
    }
}

/// Opening or reading the file, including the reloader's own reads. Only a
/// close after writing changes anything.
const fn is_read(kind: EventKind) -> bool {
    matches!(kind, EventKind::Access(access) if !matches!(access, AccessKind::Close(AccessMode::Write)))
}

async fn run(
    mut watches: Watches,
    mut events: mpsc::UnboundedReceiver<notify::Result<Event>>,
    triggers: mpsc::Sender<ReloadTrigger>,
    debounce: Duration,
) {
    let mut deadline = None;
    loop {
        tokio::select! {
            event = events.recv() => match event {
                Some(Ok(event)) => {
                    if watches.handle(&event) {
                        deadline = Some(Instant::now() + debounce);
                    }
                }
                Some(Err(error)) => {
                    tracing::warn!(%error, "config watcher error, checking the file again");
                    watches.retarget();
                    deadline = Some(Instant::now() + debounce);
                }
                None => return,
            },
            () = sleep_until(deadline.unwrap_or_else(Instant::now)), if deadline.is_some() => {
                deadline = None;
                let _ = triggers.try_send(ReloadTrigger::FileChanged);
            }
        }
    }
}

#[cfg(test)]
mod tests;
