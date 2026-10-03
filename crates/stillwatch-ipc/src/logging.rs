//! Shared `tracing` setup for the Stillwatch binaries.
//!
//! The daemon logs to the systemd journal when systemd connected its stderr to
//! the journal, and to stderr otherwise. The CLI and GUI always log to stderr.

use std::env;
use std::fs::File;
use std::io::{self, IsTerminal};
use std::os::fd::AsFd;
use std::os::unix::fs::MetadataExt;

use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::{SubscriberInitExt, TryInitError};
use tracing_subscriber::{EnvFilter, fmt};

/// Level used when neither a flag, `RUST_LOG`, nor the config sets one.
pub const DEFAULT_LEVEL: &str = "info";

/// Values accepted by the `--log-level` flags.
pub const LEVELS: [&str; 6] = ["off", "error", "warn", "info", "debug", "trace"];

/// Environment variable systemd sets to `<dev>:<inode>` of the journal stream
/// it connected to stdout/stderr.
pub const JOURNAL_STREAM_VAR: &str = "JOURNAL_STREAM";

/// Where the caller would like logs to go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogTarget {
    /// Journal when running under systemd, stderr otherwise.
    #[default]
    Auto,
    /// Always stderr.
    Stderr,
    /// Always the journal; fails if journald isn't reachable.
    Journald,
}

/// Where logs actually ended up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sink {
    /// The systemd journal.
    Journald,
    /// A human-readable formatter on stderr.
    Stderr,
}

/// Errors from [`init`] and [`init_with_override`].
#[derive(Debug, thiserror::Error)]
pub enum LoggingError {
    /// The resolved filter directives didn't parse.
    #[error("invalid log filter {directives:?}: {source}")]
    InvalidFilter {
        /// The directives that failed to parse.
        directives: String,
        /// The parser's complaint.
        #[source]
        source: tracing_subscriber::filter::ParseError,
    },
    /// [`LogTarget::Journald`] was requested but journald isn't reachable.
    #[error("can't connect to the systemd journal: {0}")]
    Journald(#[source] io::Error),
    /// A global subscriber was already installed.
    #[error("logging is already initialized")]
    AlreadyInitialized(#[source] TryInitError),
}

/// Picks the filter directives: `flag` beats `rust_log`, which beats
/// `configured`. Blank values are treated as unset.
#[must_use]
pub fn filter_directives<'a>(
    flag: Option<&'a str>,
    rust_log: Option<&'a str>,
    configured: &'a str,
) -> &'a str {
    [flag, rust_log]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|directives| !directives.is_empty())
        .unwrap_or(configured)
}

/// Decides the sink for `target`.
///
/// For [`LogTarget::Auto`] the journal is chosen only when `journal_stream`
/// (the value of `JOURNAL_STREAM`) names the same device and inode as stderr.
/// Processes started from a shell that inherited the variable keep logging to
/// stderr.
#[must_use]
pub fn select_sink(
    target: LogTarget,
    journal_stream: Option<&str>,
    stderr_id: Option<(u64, u64)>,
) -> Sink {
    match target {
        LogTarget::Stderr => Sink::Stderr,
        LogTarget::Journald => Sink::Journald,
        LogTarget::Auto => match (journal_stream.and_then(parse_journal_stream), stderr_id) {
            (Some(stream), Some(stderr)) if stream == stderr => Sink::Journald,
            _ => Sink::Stderr,
        },
    }
}

/// Installs the global subscriber at `level`, letting `RUST_LOG` override it.
///
/// # Errors
///
/// See [`init_with_override`].
pub fn init(level: &str, target: LogTarget) -> Result<Sink, LoggingError> {
    init_with_override(None, level, target)
}

/// Installs the global subscriber. `flag` (a `--log-level` value) beats
/// `RUST_LOG`, which beats `level` (from the config, or [`DEFAULT_LEVEL`]).
///
/// Returns the sink in use. With [`LogTarget::Auto`], an unreachable journal
/// falls back to stderr.
///
/// # Errors
///
/// Fails if the directives don't parse, if [`LogTarget::Journald`] can't reach
/// journald, or if logging was already initialized.
pub fn init_with_override(
    flag: Option<&str>,
    level: &str,
    target: LogTarget,
) -> Result<Sink, LoggingError> {
    let rust_log = env::var(EnvFilter::DEFAULT_ENV).ok();
    let directives = filter_directives(flag, rust_log.as_deref(), level);
    let filter = EnvFilter::try_new(directives).map_err(|source| LoggingError::InvalidFilter {
        directives: directives.to_owned(),
        source,
    })?;

    let journal_stream = env::var(JOURNAL_STREAM_VAR).ok();
    let wanted = select_sink(target, journal_stream.as_deref(), stderr_id());
    let (journald, sink) = journald_layer(wanted, target, tracing_journald::layer)?;
    let stderr = (sink == Sink::Stderr).then(|| {
        fmt::layer()
            .with_writer(io::stderr)
            .with_ansi(io::stderr().is_terminal())
    });

    tracing_subscriber::registry()
        .with(filter)
        .with(journald)
        .with(stderr)
        .try_init()
        .map_err(LoggingError::AlreadyInitialized)?;
    Ok(sink)
}

fn journald_layer<L>(
    wanted: Sink,
    target: LogTarget,
    connect: impl FnOnce() -> io::Result<L>,
) -> Result<(Option<L>, Sink), LoggingError> {
    if wanted == Sink::Stderr {
        return Ok((None, Sink::Stderr));
    }
    match connect() {
        Ok(layer) => Ok((Some(layer), Sink::Journald)),
        Err(_) if target == LogTarget::Auto => Ok((None, Sink::Stderr)),
        Err(err) => Err(LoggingError::Journald(err)),
    }
}

fn parse_journal_stream(value: &str) -> Option<(u64, u64)> {
    let (dev, ino) = value.trim().split_once(':')?;
    Some((dev.parse().ok()?, ino.parse().ok()?))
}

fn stderr_id() -> Option<(u64, u64)> {
    let fd = io::stderr().as_fd().try_clone_to_owned().ok()?;
    let metadata = File::from(fd).metadata().ok()?;
    Some((metadata.dev(), metadata.ino()))
}

#[cfg(test)]
mod tests;
