//! Client proxy for the daemon's `io.github.jslay88.Stillwatch1` interface.
//!
//! The daemon implements the server side with `#[zbus::interface(name =
//! "io.github.jslay88.Stillwatch1")]` using exactly these member names and
//! signatures:
//!
//! | Member | Signature | Notes |
//! | -- | -- | -- |
//! | `Status()` | `-> s` | JSON [`StatusPayload`](crate::status::StatusPayload) |
//! | `Snooze(t seconds)` | | |
//! | `CancelSnooze()` | | |
//! | `Pause()` | | |
//! | `Resume()` | | |
//! | `Reload()` | `-> (b ok, as errors)` | same result as the `ConfigChanged` signal |
//! | `History(t since_seconds)` | `-> s` | JSON lines of `HistoryEntry`, newer than `since_seconds` ago; 0 = all |
//! | `StartProbe(u interval_ms)` | | starts `ProbeSample` signals |
//! | `StopProbe()` | | |
//! | `PromptAnswer(s kind, u minutes)` | | see [`crate::prompt`] |
//! | `Outputs()` | `-> as` | connected output names |
//! | `Gamepads()` | `-> s` | JSON array of [`GamepadInfo`](crate::gamepad::GamepadInfo) |
//! | `Players()` | `-> as` | MPRIS player names |
//! | signal `StateChanged(s state)` | | `State::as_str` name |
//! | signal `ConfigChanged(b ok, as errors)` | | after every reload attempt |
//! | signal `ProbeSample(s json)` | | JSON [`ProbeSample`](crate::probe::ProbeSample) |

// zbus generates signal argument structs and accessors without doc comments.
#![allow(missing_docs)]

/// The Stillwatch control interface.
#[zbus::proxy(
    interface = "io.github.jslay88.Stillwatch1",
    default_service = "io.github.jslay88.Stillwatch",
    default_path = "/io/github/jslay88/Stillwatch"
)]
pub trait Stillwatch {
    /// Current status as a JSON `StatusPayload`.
    fn status(&self) -> zbus::Result<String>;

    /// Snoozes for `seconds`.
    fn snooze(&self, seconds: u64) -> zbus::Result<()>;

    /// Ends a snooze early.
    fn cancel_snooze(&self) -> zbus::Result<()>;

    /// Pauses Stillwatch until resumed.
    fn pause(&self) -> zbus::Result<()>;

    /// Undoes a pause.
    fn resume(&self) -> zbus::Result<()>;

    /// Reloads the config. Returns whether it was valid and any errors.
    fn reload(&self) -> zbus::Result<(bool, Vec<String>)>;

    /// History newer than `since_seconds` ago (0 = all), as JSON lines.
    fn history(&self, since_seconds: u64) -> zbus::Result<String>;

    /// Starts emitting `ProbeSample` signals every `interval_ms`.
    fn start_probe(&self, interval_ms: u32) -> zbus::Result<()>;

    /// Stops a running probe.
    fn stop_probe(&self) -> zbus::Result<()>;

    /// Answers the open prompt (`kind` is `snooze`, `cancel`, `timeout`, or
    /// `dismissed`; `minutes` only matters for `snooze`).
    fn prompt_answer(&self, kind: &str, minutes: u32) -> zbus::Result<()>;

    /// Connected output names.
    fn outputs(&self) -> zbus::Result<Vec<String>>;

    /// Detected gamepads as a JSON array of `GamepadInfo`.
    fn gamepads(&self) -> zbus::Result<String>;

    /// MPRIS player names currently on the bus.
    fn players(&self) -> zbus::Result<Vec<String>>;

    /// The daemon moved to `state`.
    #[zbus(signal)]
    fn state_changed(&self, state: &str) -> zbus::Result<()>;

    /// A config reload was attempted.
    #[zbus(signal)]
    fn config_changed(&self, ok: bool, errors: Vec<String>) -> zbus::Result<()>;

    /// A probe sample as JSON `ProbeSample`.
    #[zbus(signal)]
    fn probe_sample(&self, json: &str) -> zbus::Result<()>;
}
