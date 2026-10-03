//! Recent `stillwatch.service` journal lines, and opening the full log.
//!
//! Tests call [`capture`] and [`open_with`] with a stand-in program. They do
//! not run `journalctl` against the user journal.

use std::io::ErrorKind;
use std::process::Command;

const UNIT: &str = "stillwatch.service";

/// Arguments for the last 50 lines.
pub const RECENT_ARGS: [&str; 8] = [
    "--user",
    "-u",
    UNIT,
    "-n",
    "50",
    "--no-pager",
    "-o",
    "short",
];

/// Arguments a terminal runs for the full log.
pub const FULL_ARGS: [&str; 5] = ["journalctl", "--user", "-u", UNIT, "-e"];

const TERMINALS: &[(&str, &[&str])] = &[
    ("xdg-terminal-exec", &["--"]),
    ("konsole", &["-e"]),
    ("xterm", &["-e"]),
    ("alacritty", &["-e"]),
    ("kitty", &["-e"]),
    ("foot", &["-e"]),
    ("kgx", &["-e"]),
];

/// Last 50 journal lines, or one line explaining why they couldn't be read.
#[must_use]
pub fn recent_lines() -> Vec<String> {
    command_lines("journalctl", &RECENT_ARGS)
}

/// Opens the full user journal for the unit in a terminal.
///
/// # Errors
///
/// Returns an error when no terminal can be started.
pub fn open_journal() -> Result<(), String> {
    open_with(TERMINALS, &FULL_ARGS)
}

/// Runs `program` and splits stdout into lines.
#[must_use]
pub fn command_lines(program: &str, args: &[&str]) -> Vec<String> {
    match capture(program, args) {
        Ok(text) => parse_lines(&text),
        Err(err) => vec![if err.is_empty() {
            "journalctl failed".to_owned()
        } else {
            err
        }],
    }
}

/// Non-empty trimmed lines from `text`.
#[must_use]
pub fn parse_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Runs the first candidate that exists.
///
/// `prefix` is the terminal's "run this" flag. `journal` is the command.
///
/// # Errors
///
/// Returns an error when every candidate is missing or one fails to spawn
/// for a reason other than not being installed.
pub fn open_with(candidates: &[(&str, &[&str])], journal: &[&str]) -> Result<(), String> {
    for (program, prefix) in candidates {
        let mut args = Vec::with_capacity(prefix.len() + journal.len());
        args.extend(prefix.iter().map(|part| (*part).to_owned()));
        args.extend(journal.iter().map(|part| (*part).to_owned()));
        match spawn_detached(program, &args) {
            Ok(()) => return Ok(()),
            Err(err) if err.kind() == ErrorKind::NotFound => {}
            Err(err) => return Err(err.to_string()),
        }
    }
    Err("No terminal found to show the journal.".to_owned())
}

/// Stdout of a successful run, or stderr (possibly empty) when it fails.
///
/// # Errors
///
/// Returns the spawn error, or stderr when the process exits non-zero.
pub fn capture(program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|err| err.to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&output.stderr);
        Err(err.trim().to_owned())
    }
}

fn spawn_detached(program: &str, args: &[String]) -> std::io::Result<()> {
    let mut child = Command::new(program).args(args).spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}
