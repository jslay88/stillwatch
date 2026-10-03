//! Shell state: which windows are open, which page is showing, and what the
//! daemon last told us. No I/O.

use std::path::PathBuf;

use stillwatch_core::state::State;
use stillwatch_ipc::status::StatusPayload;

use crate::edit_msg::SettingsMsg;
use crate::launch::LaunchMode;
use crate::page::Page;
use crate::settings::Editor;

/// Whether a window is closed, open, or open and needing focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Visibility {
    /// The window is not shown.
    #[default]
    Closed,
    /// The window is shown.
    Open,
    /// The window is shown and should be brought forward.
    Focus,
}

impl Visibility {
    /// The window exists on screen.
    #[must_use]
    pub const fn is_open(self) -> bool {
        matches!(self, Self::Open | Self::Focus)
    }

    /// Drops a one-shot focus request once the view layer has applied it.
    #[must_use]
    pub const fn opened(self) -> Self {
        match self {
            Self::Focus => Self::Open,
            other => other,
        }
    }

    /// Opens a closed window, or focuses one that is already open.
    #[must_use]
    pub const fn reveal(self) -> Self {
        if self.is_open() {
            Self::Focus
        } else {
            Self::Open
        }
    }
}

/// Which shell window a focus or close refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    /// The settings window (pages live inside it).
    Settings,
    /// The prompt placeholder.
    Prompt,
}

/// What the tray and status line know about the daemon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    /// Nothing owns the daemon's bus name.
    Down,
    /// The daemon answered with this snapshot.
    Up(Snapshot),
}

/// The daemon facts the shell renders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// Current state machine state.
    pub state: State,
    /// Seconds left on a snooze, when the daemon reported one.
    pub snooze_remaining_seconds: Option<u64>,
    /// Reload problems from `Status()`, each `key: message` when the key is known.
    pub config_errors: Vec<String>,
}

impl Snapshot {
    /// The fields of a `Status()` payload the shell displays.
    #[must_use]
    pub fn from_status(status: &StatusPayload) -> Self {
        Self {
            state: status.state,
            snooze_remaining_seconds: status.snooze_remaining_seconds,
            config_errors: status.config_errors.clone(),
        }
    }
}

/// Something the daemon watcher observed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DaemonEvent {
    /// The daemon name has no owner.
    Down,
    /// A fresh status payload.
    Snapshot(Snapshot),
    /// `StateChanged` arrived and a status refresh didn't.
    State(State),
    /// `ConfigChanged`.
    Config {
        /// Whether the reload was accepted.
        ok: bool,
        /// Problems from a rejected reload.
        errors: Vec<String>,
    },
    /// Snooze presets read after a successful reload.
    Presets(Vec<u32>),
    /// A tray action's D-Bus call failed.
    CallFailed(String),
}

/// A method the tray asked the daemon to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaemonCall {
    /// `Snooze` for this many seconds.
    Snooze {
        /// Duration in seconds.
        seconds: u64,
    },
    /// `CancelSnooze`.
    CancelSnooze,
    /// `Pause`.
    Pause,
    /// `Resume`.
    Resume,
    /// `Reload()`. Skipped by the shell when the daemon is down.
    Reload,
}

/// A tray menu (or left-click) choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    /// Quick snooze of this many minutes.
    Snooze {
        /// Preset length in minutes.
        minutes: u32,
    },
    /// End the current snooze.
    CancelSnooze,
    /// Pause monitoring.
    Pause,
    /// Resume monitoring.
    Resume,
    /// Open the settings window.
    OpenSettings,
    /// Leave the tray.
    Quit,
}

/// An input to [`update`](crate::model::update).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    /// Show another page of the settings window.
    Navigate(Page),
    /// Open or focus the settings window.
    OpenSettings,
    /// The settings window went away.
    CloseSettings,
    /// Open or focus the prompt placeholder.
    OpenPrompt,
    /// The prompt window went away.
    ClosePrompt,
    /// This process should exit.
    Quit,
    /// A tray menu item was activated.
    Tray(TrayAction),
    /// This process is the one that owns the GUI bus name, or the bus is
    /// missing and the window should open anyway.
    BecamePrimary {
        /// `true` for the first start, `false` for a second invocation.
        first: bool,
        /// What that start asked to show.
        mode: LaunchMode,
    },
    /// News from the daemon watcher.
    Daemon(DaemonEvent),
    /// An edit, save, or restore on the settings page.
    Settings(SettingsMsg),
}

/// The settings window, the prompt placeholder, and the daemon link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shell {
    /// Page shown inside the settings window.
    pub page: Page,
    /// Settings window visibility.
    pub settings: Visibility,
    /// Prompt placeholder visibility.
    pub prompt: Visibility,
    /// Daemon presence and last snapshot.
    pub link: Link,
    /// `[prompt] snooze_presets_minutes`, for the tray menu.
    pub presets_minutes: Vec<u32>,
    /// `ConfigChanged.ok`, once a reload has been seen.
    pub config_ok: Option<bool>,
    /// `ConfigChanged` errors from the last rejected reload.
    pub config_errors: Vec<String>,
    /// The last daemon call that failed, for the status line.
    pub notice: Option<String>,
    /// Tray quit, or the window's Quit button.
    pub quit: bool,
    /// Config file the settings page reads and writes.
    pub config_path: Option<PathBuf>,
    /// Settings form. Defaults until a file is loaded.
    pub editor: Editor,
}

impl Shell {
    /// A closed shell on `presets_minutes`, with the daemon treated as down.
    #[must_use]
    pub fn new(presets_minutes: Vec<u32>) -> Self {
        Self {
            page: Page::Settings,
            settings: Visibility::Closed,
            prompt: Visibility::Closed,
            link: Link::Down,
            presets_minutes,
            config_ok: None,
            config_errors: Vec::new(),
            notice: None,
            quit: false,
            config_path: None,
            editor: Editor::pristine(),
        }
    }

    /// Opens the window `mode` asks for.
    ///
    /// The first start of [`LaunchMode::Tray`] leaves both windows closed.
    /// Any later activation, including tray, brings the matching window
    /// forward. A window that is already open is focused instead of duplicated.
    pub fn apply_launch(&mut self, mode: LaunchMode, first: bool) {
        match mode {
            LaunchMode::Tray if first => {}
            LaunchMode::Tray | LaunchMode::Settings => {
                self.settings = self.settings.reveal();
            }
            LaunchMode::Prompt => {
                self.prompt = self.prompt.reveal();
            }
        }
    }

    /// Forgets focus requests the view layer has already applied.
    pub fn settle_focus(&mut self) {
        self.settings = self.settings.opened();
        self.prompt = self.prompt.opened();
    }
}

/// Minutes to the seconds `Snooze` takes.
#[must_use]
pub fn snooze_seconds(minutes: u32) -> u64 {
    u64::from(minutes).saturating_mul(60)
}
