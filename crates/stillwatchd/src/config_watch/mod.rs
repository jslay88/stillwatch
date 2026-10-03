//! Hot reload of `config.toml`: the directory watcher, the reloader that
//! keeps the last good config, and the single reload path every trigger
//! goes through.
//!
//! The daemon's loop waits on three triggers: [`ConfigWatcher::next`] (a
//! debounced change on disk), SIGHUP ([`crate::signals::Signal::reload_trigger`]),
//! and D-Bus `Reload()`. Each one goes to [`reload_and_report`], which reloads,
//! emits `ConfigChanged` once for the attempt, and returns a
//! [`ReloadOutcome`] that says what the daemon has to apply.

mod reloader;
mod report;
mod targets;
mod watcher;

pub use reloader::{Applied, ReloadOutcome, Reloader, error_messages};
pub use report::{ReloadSignal, reload_and_report};
pub use watcher::{ConfigWatcher, DEFAULT_DEBOUNCE, WatchError};

/// What asked for a reload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReloadTrigger {
    /// The watcher saw the config file, a symlink to it, or its directory
    /// change. Skipped when the contents hash the same as last time.
    FileChanged,
    /// SIGHUP, which `systemctl --user reload stillwatch` sends.
    Hangup,
    /// The D-Bus `Reload()` method.
    Requested,
}

impl ReloadTrigger {
    /// Whether the reload runs even when the file hasn't changed. Only the
    /// watcher's own triggers can be skipped.
    #[must_use]
    pub const fn is_forced(self) -> bool {
        !matches!(self, Self::FileChanged)
    }
}

#[cfg(test)]
mod tests {
    use super::ReloadTrigger;

    #[test]
    fn only_file_changes_can_be_skipped() {
        assert!(!ReloadTrigger::FileChanged.is_forced());
        assert!(ReloadTrigger::Hangup.is_forced());
        assert!(ReloadTrigger::Requested.is_forced());
    }
}
