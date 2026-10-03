//! Tray icon, tooltip, and menu derived from a [`Shell`](crate::shell::Shell).

use stillwatch_core::state::State;

use crate::shell::{Link, Shell, Snapshot, TrayAction};

/// Which pixmap the status icon shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrayIcon {
    /// The daemon's bus name has no owner.
    Down,
    /// [`State::Active`](stillwatch_core::state::State::Active).
    Active,
    /// [`State::Monitoring`](stillwatch_core::state::State::Monitoring).
    Monitoring,
    /// [`State::Prompting`](stillwatch_core::state::State::Prompting).
    Prompting,
    /// [`State::Snoozed`](stillwatch_core::state::State::Snoozed).
    Snoozed,
    /// [`State::Acting`](stillwatch_core::state::State::Acting).
    Acting,
    /// [`State::Blanked`](stillwatch_core::state::State::Blanked).
    Blanked,
    /// [`State::Locked`](stillwatch_core::state::State::Locked).
    Locked,
    /// [`State::Paused`](stillwatch_core::state::State::Paused).
    Paused,
}

impl TrayIcon {
    /// Every icon, in the order tests check they differ.
    #[cfg(test)]
    pub const ALL: [Self; 9] = [
        Self::Down,
        Self::Active,
        Self::Monitoring,
        Self::Prompting,
        Self::Snoozed,
        Self::Acting,
        Self::Blanked,
        Self::Locked,
        Self::Paused,
    ];

    /// sRGB of the disc. Daemon-down is drawn as a ring instead.
    #[must_use]
    pub const fn rgb(self) -> (u8, u8, u8) {
        match self {
            Self::Down => (140, 140, 148),
            Self::Active => (48, 168, 88),
            Self::Monitoring => (48, 112, 196),
            Self::Prompting => (214, 148, 32),
            Self::Snoozed => (132, 84, 196),
            Self::Acting => (214, 96, 40),
            Self::Blanked => (36, 40, 52),
            Self::Locked => (184, 56, 64),
            Self::Paused => (196, 176, 48),
        }
    }
}

/// One row of the tray menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuEntry {
    /// Text the menu shows.
    pub label: String,
    /// What activating the row does.
    pub action: TrayAction,
}

/// Icon, tooltip, and menu for the current shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayModel {
    /// Pixmap to show.
    pub icon: TrayIcon,
    /// Tooltip title.
    pub title: String,
    /// Tooltip body: state, and snooze time left when we have it.
    pub description: String,
    /// Menu rows, top to bottom.
    pub entries: Vec<MenuEntry>,
}

/// The icon for `link`.
#[must_use]
pub fn icon_for(link: &Link) -> TrayIcon {
    match link {
        Link::Down => TrayIcon::Down,
        Link::Up(snapshot) => icon_for_state(snapshot.state),
    }
}

/// The icon for a daemon `state`.
#[must_use]
pub const fn icon_for_state(state: State) -> TrayIcon {
    match state {
        State::Active => TrayIcon::Active,
        State::Monitoring => TrayIcon::Monitoring,
        State::Prompting => TrayIcon::Prompting,
        State::Snoozed => TrayIcon::Snoozed,
        State::Acting => TrayIcon::Acting,
        State::Blanked => TrayIcon::Blanked,
        State::Locked => TrayIcon::Locked,
        State::Paused => TrayIcon::Paused,
    }
}

/// Status-line and tooltip body.
#[must_use]
pub fn status_text(link: &Link) -> String {
    match link {
        Link::Down => "stillwatchd is not running".to_owned(),
        Link::Up(snapshot) => describe(snapshot),
    }
}

/// `seconds` as `1h 2m`, `3m 4s`, or `5s`.
#[must_use]
pub fn format_remaining(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;
    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m {secs}s")
    } else {
        format!("{secs}s")
    }
}

/// Tray contents for `shell`.
#[must_use]
pub fn tray_model(shell: &Shell) -> TrayModel {
    TrayModel {
        icon: icon_for(&shell.link),
        title: "Stillwatch".to_owned(),
        description: status_text(&shell.link),
        entries: menu(shell),
    }
}

fn describe(snapshot: &Snapshot) -> String {
    let state = snapshot.state.as_str();
    match snapshot.snooze_remaining_seconds {
        Some(seconds) => format!("{state}, {} left", format_remaining(seconds)),
        None => state.to_owned(),
    }
}

fn menu(shell: &Shell) -> Vec<MenuEntry> {
    let mut entries = Vec::new();
    for minutes in &shell.presets_minutes {
        entries.push(MenuEntry {
            label: snooze_label(*minutes),
            action: TrayAction::Snooze { minutes: *minutes },
        });
    }
    entries.push(entry("Cancel snooze", TrayAction::CancelSnooze));
    entries.push(pause_or_resume(&shell.link));
    entries.push(entry("Settings", TrayAction::OpenSettings));
    entries.push(entry("Quit", TrayAction::Quit));
    entries
}

fn pause_or_resume(link: &Link) -> MenuEntry {
    match link {
        Link::Up(snapshot) if snapshot.state == State::Paused => {
            entry("Resume", TrayAction::Resume)
        }
        _ => entry("Pause", TrayAction::Pause),
    }
}

fn snooze_label(minutes: u32) -> String {
    if minutes == 1 {
        "Snooze 1 minute".to_owned()
    } else {
        format!("Snooze {minutes} minutes")
    }
}

fn entry(label: &str, action: TrayAction) -> MenuEntry {
    MenuEntry {
        label: label.to_owned(),
        action,
    }
}

#[cfg(test)]
mod tests;
