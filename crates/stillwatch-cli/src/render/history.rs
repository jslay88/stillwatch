//! `stillwatch history`: one table row per decision.

use stillwatch_core::command::BlankMethod;
use stillwatch_core::history::{
    HistoryEntry, HistoryKind, PromptAnswer, PromptMedium, PromptReason,
};
use stillwatch_core::stats::DetectionStats;

use super::{Style, duration, percent, threshold};

const HEADER: [&str; COLUMNS] = ["TIME", "EVENT", "STATES", "DETECTION", "DETAIL", "CONTEXT"];
const COLUMNS: usize = 6;
const EMPTY: &str = "-";

/// Renders entries as an aligned table, oldest first, in the order given.
///
/// ```text
/// TIME                 EVENT            STATES                   DETECTION                            DETAIL       CONTEXT
/// 2026-10-02 20:50:13  transition       monitoring -> prompting  HDMI-A-1 72% (threshold 70% normal)  -            media
/// 2026-10-02 20:51:13  prompt answered  -                        -                                    snooze, 45m  -
/// ```
#[must_use]
pub fn render(entries: &[HistoryEntry], style: &Style) -> String {
    if entries.is_empty() {
        return "no history entries\n".to_owned();
    }
    let mut rows = vec![HEADER.map(str::to_owned)];
    rows.extend(entries.iter().map(|entry| row(entry, style)));
    let mut widths = [0; COLUMNS];
    for row in &rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }
    let mut out = String::new();
    for row in &rows {
        let cells: Vec<String> = row
            .iter()
            .zip(widths)
            .map(|(cell, width)| format!("{cell:<width$}"))
            .collect();
        out.push_str(cells.join("  ").trim_end());
        out.push('\n');
    }
    out
}

fn row(entry: &HistoryEntry, style: &Style) -> [String; COLUMNS] {
    [
        style.time(entry.at),
        event(entry.kind).to_owned(),
        states(entry),
        entry
            .detection
            .as_ref()
            .map_or_else(|| EMPTY.to_owned(), detection),
        or_empty(&details(entry)),
        or_empty(&context(entry)),
    ]
}

const fn event(kind: HistoryKind) -> &'static str {
    match kind {
        HistoryKind::Transition => "transition",
        HistoryKind::Prompt => "prompt shown",
        HistoryKind::Snooze => "snooze",
        HistoryKind::Blank => "blank",
        HistoryKind::Reblank => "re-blank",
        HistoryKind::Ceiling => "ceiling",
        HistoryKind::ConfigReload => "config reload",
        HistoryKind::OverlayUsed => "overlay used",
        HistoryKind::PromptAnswered => "prompt answered",
        HistoryKind::ConfigReloadFailed => "reload failed",
        HistoryKind::Migration => "config migrated",
    }
}

fn states(entry: &HistoryEntry) -> String {
    match (entry.from, entry.to) {
        (Some(from), Some(to)) => format!("{from} -> {to}"),
        (Some(from), None) => format!("from {from}"),
        (None, Some(to)) => format!("to {to}"),
        (None, None) => EMPTY.to_owned(),
    }
}

/// `HDMI-A-1 72%, DP-1 40% (threshold 70% normal)`
fn detection(detection: &DetectionStats) -> String {
    let outputs: Vec<String> = detection
        .outputs
        .iter()
        .map(|stats| format!("{} {}", stats.output, percent(stats.persistent_percent)))
        .collect();
    let applied = format!("threshold {}", threshold(detection.threshold));
    if outputs.is_empty() {
        applied
    } else {
        format!("{} ({applied})", outputs.join(", "))
    }
}

fn details(entry: &HistoryEntry) -> Vec<String> {
    let mut parts = Vec::new();
    if let Some(style) = entry.prompt_style {
        parts.push(medium_name(style).to_owned());
    }
    if let Some(reason) = entry.prompt_reason {
        parts.push(reason_name(reason).to_owned());
    }
    if let Some(answer) = entry.answer {
        parts.push(answer_name(answer).to_owned());
    }
    if let Some(seconds) = entry.snooze_seconds {
        parts.push(duration(seconds));
    }
    if let Some(method) = entry.blank_method {
        parts.push(format!("via {}", method_name(method)));
    }
    if let Some(attempt) = entry.reblank_attempt {
        parts.push(format!("attempt {attempt}"));
    }
    if let Some(count) = entry.error_count {
        let noun = if count == 1 { "problem" } else { "problems" };
        parts.push(format!("{count} {noun}"));
    }
    if let (Some(from), Some(to)) = (entry.from_version, entry.to_version) {
        parts.push(format!("v{from} -> v{to}"));
    }
    parts
}

const fn medium_name(style: PromptMedium) -> &'static str {
    match style {
        PromptMedium::Notification => "notification",
        PromptMedium::Dialog => "dialog",
    }
}

const fn reason_name(reason: PromptReason) -> &'static str {
    match reason {
        PromptReason::Configured => "configured",
        PromptReason::Auto => "auto",
        PromptReason::Fullscreen => "fullscreen",
        PromptReason::FallbackUnavailable => "no notification server",
        PromptReason::FallbackFailed => "notification failed",
        PromptReason::FallbackClosed => "closed without an action",
    }
}

const fn answer_name(answer: PromptAnswer) -> &'static str {
    match answer {
        PromptAnswer::Snooze => "snooze",
        PromptAnswer::Custom => "custom",
        PromptAnswer::Cancel => "cancel",
        PromptAnswer::Timeout => "timeout",
        PromptAnswer::Dismissed => "dismissed",
        PromptAnswer::Failed => "failed",
    }
}

const fn method_name(method: BlankMethod) -> &'static str {
    match method {
        BlankMethod::Dpms => "dpms",
        BlankMethod::Overlay => "overlay",
        BlankMethod::DdcStandby => "ddc standby",
    }
}

fn context(entry: &HistoryEntry) -> Vec<String> {
    let flags = [
        (entry.context.media_playing, "media"),
        (entry.context.gamepad_active, "gamepad"),
        (entry.context.locked, "locked"),
    ];
    flags
        .into_iter()
        .filter(|(set, _)| *set)
        .map(|(_, name)| name.to_owned())
        .collect()
}

fn or_empty(parts: &[String]) -> String {
    if parts.is_empty() {
        EMPTY.to_owned()
    } else {
        parts.join(", ")
    }
}

#[cfg(test)]
mod tests;
