//! `stillwatch status`: one labelled line per fact.

use stillwatch_core::stats::DetectionStats;
use stillwatch_ipc::status::{PanelCareStatus, StatusPayload};

use super::{Style, duration, output_summary, percent, threshold, verdict};

const LABEL_WIDTH: usize = 12;

/// Renders the status as aligned `label  value` lines.
///
/// ```text
/// state       snoozed for 4m 12s
/// snooze      40m 48s left
/// idle        yes
/// locked      no
/// media       not playing
/// capture     kwin
/// last check  STALE, threshold 70% normal
///   HDMI-A-1: persistent 72% (dark 18%, counted 82%), threshold 70% normal -> STALE
/// panel care  screen on 3h, last standby 2026-10-02 18:00:00, overlay used 2 times
/// config      ok
/// ```
#[must_use]
pub fn render(status: &StatusPayload, style: &Style) -> String {
    let mut lines = vec![line(
        "state",
        &format!("{} for {}", status.state, duration(status.state_seconds)),
    )];
    if let Some(left) = status.snooze_remaining_seconds {
        lines.push(line("snooze", &format!("{} left", duration(left))));
    }
    lines.push(line("idle", yes_no(status.idle)));
    lines.push(line("locked", yes_no(status.locked)));
    let media = if status.media_playing {
        "playing"
    } else {
        "not playing"
    };
    lines.push(line("media", media));
    lines.extend(backends(status));
    lines.extend(last_check(status.last_detection.as_ref(), style));
    if let Some(panel) = &status.panel_care {
        lines.push(line("panel care", &panel_care(panel, style)));
    }
    lines.extend(config(&status.config_errors));
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

fn backends(status: &StatusPayload) -> Vec<String> {
    let Some(report) = &status.backends else {
        let capture = status
            .capture_backend
            .as_deref()
            .unwrap_or("none (input idle only)");
        return vec![line("capture", capture)];
    };
    vec![
        line("idle source", &report.idle),
        line(
            "capture",
            &with_why(&report.capture, &report.capture_reason),
        ),
        line("blank", &with_why(&report.blank, &report.blank_reason)),
        line("prompt", &with_why(&report.prompt, &report.prompt_reason)),
    ]
}

fn with_why(name: &str, why: &str) -> String {
    format!("{name} ({why})")
}

fn line(label: &str, value: &str) -> String {
    format!("{label:<LABEL_WIDTH$}{value}")
}

const fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn last_check(detection: Option<&DetectionStats>, style: &Style) -> Vec<String> {
    let Some(detection) = detection else {
        return vec![line("last check", "none yet")];
    };
    let summary = format!(
        "{}, threshold {}",
        verdict(detection.stale, style),
        threshold(detection.threshold)
    );
    let mut lines = vec![line("last check", &summary)];
    lines.extend(detection.outputs.iter().map(|stats| {
        let counted = percent(stats.counted_percent);
        format!(
            "  {}",
            output_summary(stats, &counted, detection.threshold, style)
        )
    }));
    lines
}

fn panel_care(panel: &PanelCareStatus, style: &Style) -> String {
    let standby = panel
        .last_standby
        .map_or_else(|| "never".to_owned(), |at| style.time(at));
    let times = if panel.overlay_uses == 1 {
        "time"
    } else {
        "times"
    };
    format!(
        "screen on {}, last standby {standby}, overlay used {} {times}",
        duration(panel.screen_on_seconds),
        panel.overlay_uses
    )
}

fn config(errors: &[String]) -> Vec<String> {
    if errors.is_empty() {
        return vec![line("config", "ok")];
    }
    let noun = if errors.len() == 1 {
        "problem"
    } else {
        "problems"
    };
    let summary = format!(
        "the last reload failed ({} {noun}); the last good config is still in effect",
        errors.len()
    );
    let mut lines = vec![line("config", &summary)];
    lines.extend(errors.iter().map(|error| format!("  {error}")));
    lines
}

#[cfg(test)]
mod tests;
