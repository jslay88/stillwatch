//! The last good config and the load, migrate, validate path that replaces
//! it.

use std::hash::{DefaultHasher, Hash as _, Hasher as _};
use std::path::{Path, PathBuf};
use std::time::Instant;

use jiff::Timestamp;
use stillwatch_core::command::Command;
use stillwatch_core::config::{Config, ConfigChanges, ConfigError, LoadOutcome};
use stillwatch_core::state::StateMachine;
use stillwatch_ipc::config_file;

use super::ReloadTrigger;
use crate::service::ReloadReport;

/// What the file looked like at the last attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Seen {
    Missing,
    Contents(u64),
}

/// Owns the config in effect and reloads it from disk.
///
/// The file is only ever read. A migrated config is used in memory and the
/// file keeps its old `version` until the GUI saves it.
#[derive(Debug)]
pub struct Reloader {
    path: PathBuf,
    config: Config,
    from_file: bool,
    seen: Option<Seen>,
    errors: Vec<String>,
}

/// A config that loaded, validated, and is now in effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    /// The new config plus whether (and from which version) it was migrated.
    pub loaded: LoadOutcome,
    /// What differs from the config it replaced. Can be empty, for a forced
    /// reload or a file that went back to the config in effect.
    pub changes: ConfigChanges,
}

/// The result of [`Reloader::reload`].
#[derive(Debug)]
pub enum ReloadOutcome {
    /// The file hashes the same as at the last attempt; nothing was done and
    /// nothing is reported.
    Unchanged,
    /// The new config is in effect.
    Applied(Box<Applied>),
    /// The file is missing, unreadable, or invalid. The last good config
    /// stays in effect.
    Rejected(ConfigError),
}

impl Reloader {
    /// Loads the config at `path` for start-up.
    ///
    /// A missing file means defaults (the returned outcome is the default
    /// config, not migrated). Call `StateMachine::config_migrated` with the
    /// outcome after building the machine.
    ///
    /// # Errors
    ///
    /// Any other read, parse, migration, or validation error, so the daemon
    /// refuses to start on a bad config.
    pub fn load(path: PathBuf) -> Result<(Self, LoadOutcome), ConfigError> {
        let (loaded, seen) = match config_file::read(&path) {
            Ok(text) => (Config::from_toml_str(&text)?, Seen::Contents(hash(&text))),
            Err(ConfigError::NotFound { .. }) => {
                tracing::info!(path = %path.display(), "no config file, running on defaults");
                (defaults(), Seen::Missing)
            }
            Err(error) => return Err(error),
        };
        log_migration(&loaded);
        let reloader = Self {
            path,
            config: loaded.config.clone(),
            from_file: seen != Seen::Missing,
            seen: Some(seen),
            errors: Vec::new(),
        };
        Ok((reloader, loaded))
    }

    /// The config in effect.
    #[must_use]
    pub const fn config(&self) -> &Config {
        &self.config
    }

    /// The config file this reloader reads.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Errors from the last attempt if it was rejected, for `Status`. Empty
    /// once a reload applies.
    #[must_use]
    pub fn errors(&self) -> &[String] {
        &self.errors
    }

    /// Reads the file again and, if it's good, makes it the config in effect.
    ///
    /// Unless `trigger` [is forced](ReloadTrigger::is_forced), contents that
    /// hash the same as at the last attempt (good or bad) are skipped. A
    /// deleted file is rejected and the last good config stays, unless the
    /// daemon has been on defaults all along, in which case a missing file is
    /// simply still the defaults.
    pub fn reload(&mut self, trigger: ReloadTrigger) -> ReloadOutcome {
        let read = config_file::read(&self.path);
        let seen = match &read {
            Ok(text) => Some(Seen::Contents(hash(text))),
            Err(ConfigError::NotFound { .. }) => Some(Seen::Missing),
            Err(_) => None,
        };
        if !trigger.is_forced() && seen.is_some() && seen == self.seen {
            return ReloadOutcome::Unchanged;
        }
        self.seen = seen;
        let loaded = match read {
            Ok(text) => Config::from_toml_str(&text),
            Err(ConfigError::NotFound { .. }) if !self.from_file => Ok(defaults()),
            Err(error) => Err(error),
        };
        match loaded {
            Ok(loaded) => ReloadOutcome::Applied(Box::new(self.apply(loaded, seen))),
            Err(error) => {
                self.errors = error_messages(&error);
                tracing::warn!(
                    ?trigger,
                    errors = ?self.errors,
                    "config reload rejected, keeping the last good config"
                );
                ReloadOutcome::Rejected(error)
            }
        }
    }

    fn apply(&mut self, loaded: LoadOutcome, seen: Option<Seen>) -> Applied {
        log_migration(&loaded);
        let changes = ConfigChanges::between(&self.config, &loaded.config);
        tracing::info!(
            changed = ?changes.keys().collect::<Vec<_>>(),
            resets_detection = changes.resets_detection(),
            "config reloaded"
        );
        self.config = loaded.config.clone();
        self.from_file = seen != Some(Seen::Missing);
        self.errors.clear();
        Applied { loaded, changes }
    }
}

impl ReloadOutcome {
    /// What `ConfigChanged` and `Reload()` carry for this attempt. `None`
    /// for [`ReloadOutcome::Unchanged`], which isn't an attempt.
    #[must_use]
    pub fn report(&self) -> Option<ReloadReport> {
        match self {
            Self::Unchanged => None,
            Self::Applied(_) => Some(ReloadReport::applied()),
            Self::Rejected(error) => Some(ReloadReport::rejected(error_messages(error))),
        }
    }

    /// Hands the outcome to the state machine and returns its commands
    /// (history entries, re-armed timers) to execute.
    ///
    /// An applied config goes to `apply_config`, which records the reload,
    /// then to `config_migrated`, which records a migration if there was
    /// one. A rejected one goes to `config_reload_failed`.
    pub fn update_machine(
        &self,
        machine: &mut StateMachine,
        now: Instant,
        wall: Timestamp,
    ) -> Vec<Command> {
        match self {
            Self::Unchanged => Vec::new(),
            Self::Applied(applied) => {
                let mut commands = machine.apply_config(now, wall, &applied.loaded.config);
                commands.extend(machine.config_migrated(now, wall, &applied.loaded));
                commands
            }
            Self::Rejected(error) => machine.config_reload_failed(now, wall, error),
        }
    }
}

/// One message per problem: `key: message` for every keyed issue, otherwise
/// the error itself (missing file, bad TOML, unsupported version).
#[must_use]
pub fn error_messages(error: &ConfigError) -> Vec<String> {
    let issues = error.keyed_issues();
    if issues.is_empty() {
        vec![error.to_string()]
    } else {
        issues.iter().map(ToString::to_string).collect()
    }
}

fn defaults() -> LoadOutcome {
    LoadOutcome {
        config: Config::default(),
        migrated_from: None,
        notes: Vec::new(),
    }
}

fn log_migration(loaded: &LoadOutcome) {
    if let Some(from) = loaded.migrated_from {
        tracing::info!(
            from,
            to = loaded.config.version,
            notes = ?loaded.notes.iter().map(|note| &note.message).collect::<Vec<_>>(),
            "config migrated in memory; the file is left as it is"
        );
    }
}

fn hash(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests;
