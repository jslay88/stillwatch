//! `stillwatch probe --standalone`: spawn the installed `stillwatchd --probe`
//! and render its JSON lines with the shared probe grid.

use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::pin::pin;
use std::process::Stdio;

use anyhow::Context as _;
use stillwatch_ipc::json::from_json;
use stillwatch_ipc::probe::ProbeSample;
use tokio::io::{AsyncBufReadExt as _, BufReader};
use tokio::process::Command;

use super::block_on;
use super::probe;
use crate::args::ProbeArgs;
use crate::render::Style;

/// Spawns `stillwatchd --probe` and writes each sample to `out`.
///
/// The child is the `stillwatchd` next to this `stillwatch` (the same path
/// `KWin`'s `.desktop` `Exec=` has to name). `--json` forwards the lines;
/// otherwise each sample goes through [`probe::write_sample`].
///
/// # Errors
///
/// `stillwatchd` isn't next to this binary and isn't on `PATH`, it exits
/// before printing a sample, a line isn't a `ProbeSample`, or `out` fails.
pub fn run(
    args: &ProbeArgs,
    log_level: Option<&str>,
    style: &Style,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    block_on(stream(args, log_level, style, out))
}

async fn stream(
    args: &ProbeArgs,
    log_level: Option<&str>,
    style: &Style,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    let mut child = Command::from(daemon_command(args, log_level))
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| {
            format!(
                "can't start {} --probe (install stillwatchd; ScreenShot2 only authorizes that binary)",
                stillwatchd_path().display()
            )
        })?;
    let stdout = child
        .stdout
        .take()
        .context("stillwatchd stdout wasn't piped")?;
    let mut lines = BufReader::new(stdout).lines();
    let mut stop = pin!(interrupted());
    let mut seen = 0usize;
    loop {
        tokio::select! {
            () = &mut stop => {
                let _ = child.start_kill();
                break;
            }
            line = lines.next_line() => {
                match line.context("reading stillwatchd --probe")? {
                    Some(line) if line.is_empty() => {}
                    Some(line) => {
                        write_line(&line, args.json, style, out)?;
                        seen += 1;
                        if args.count.is_some_and(|count| seen >= count.get()) {
                            let _ = child.start_kill();
                            break;
                        }
                    }
                    None => break,
                }
            }
        }
    }
    let status = child.wait().await.context("waiting for stillwatchd")?;
    if !status.success() && seen == 0 {
        anyhow::bail!("stillwatchd --probe exited {status}");
    }
    Ok(())
}

/// Renders one JSON line, or forwards it when `json` is set.
///
/// # Errors
///
/// The line isn't a [`ProbeSample`], or `out` fails.
pub fn write_line(
    line: &str,
    json: bool,
    style: &Style,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    if json {
        writeln!(out, "{line}")?;
        out.flush().context("can't write the probe sample")?;
        return Ok(());
    }
    let sample: ProbeSample = from_json(line)?;
    probe::write_sample(&sample, false, style, out)
}

fn daemon_command(args: &ProbeArgs, log_level: Option<&str>) -> std::process::Command {
    let mut command = std::process::Command::new(stillwatchd_path());
    command.arg("--probe");
    command.arg("--log-level");
    command.arg(log_level.unwrap_or("warn"));
    if let Some(interval) = args.interval {
        command.arg("--interval");
        command.arg(humantime::format_duration(interval).to_string());
    }
    if let Some(count) = args.count {
        command.arg("--count");
        command.arg(count.to_string());
    }
    command
}

/// The `stillwatchd` installed next to this `stillwatch`, else the `PATH` name.
#[must_use]
pub fn stillwatchd_path() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        let mut sibling = exe;
        sibling.set_file_name("stillwatchd");
        if sibling.is_file() {
            return sibling;
        }
    }
    PathBuf::from("stillwatchd")
}

async fn interrupted() {
    if tokio::signal::ctrl_c().await.is_err() {
        std::future::pending::<()>().await;
    }
}

/// Test hook: the argv `stillwatchd --probe` would get.
#[must_use]
pub fn daemon_argv(args: &ProbeArgs, log_level: Option<&str>) -> Vec<OsString> {
    daemon_command(args, log_level)
        .get_args()
        .map(OsString::from)
        .collect()
}

#[cfg(test)]
mod tests;
