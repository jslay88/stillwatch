//! Lines for one history row. Built from [`HistoryEntry`](stillwatch_core::history::HistoryEntry)
//! here so the CLI table stays the CLI's.

use stillwatch_core::command::BlankMethod;
use stillwatch_core::history::{HistoryEntry, HistoryKind};
use stillwatch_core::stats::{OutputStats, ThresholdReason};

use super::{HistoryRow, KINDS, KindFilter, TimeRange};

pub(crate) fn row_from(entry: &HistoryEntry) -> HistoryRow {
    HistoryRow {
        at: entry.at.as_second(),
        kind: entry.kind,
        label: row_label(entry),
        detail: detail_lines(entry),
    }
}

pub(crate) fn rows_from(entries: &[HistoryEntry]) -> Vec<HistoryRow> {
    entries.iter().map(row_from).collect()
}

pub(crate) fn accepts(row: &HistoryRow, range: TimeRange, kind: KindFilter, now: i64) -> bool {
    if let KindFilter::Kind(expected) = kind
        && row.kind != expected
    {
        return false;
    }
    range.cutoff(now).is_none_or(|cutoff| row.at >= cutoff)
}

pub(crate) fn kind_label(kind: HistoryKind) -> &'static str {
    KINDS
        .iter()
        .find(|(candidate, _)| *candidate == kind)
        .map_or("event", |(_, label)| *label)
}

/// The list label and the detail block for `entry`.
pub(crate) fn detail_lines(entry: &HistoryEntry) -> Vec<String> {
    let mut lines = vec![row_label(entry)];
    match entry.detection.as_ref() {
        Some(detection) if detection.outputs.is_empty() => {
            lines.push("No per-output percentages.".to_owned());
        }
        Some(detection) => {
            for output in &detection.outputs {
                lines.push(output_line(output));
            }
            lines.push(format!(
                "Threshold {}% ({})",
                detection.threshold.percent,
                threshold_why(detection.threshold.reason),
            ));
        }
        None => {}
    }
    lines.push(flag(
        "Media",
        entry.context.media_playing,
        "playing",
        "not playing",
    ));
    lines.push(flag(
        "Gamepad",
        entry.context.gamepad_active,
        "active",
        "idle",
    ));
    lines.push(flag("Session", entry.context.locked, "locked", "unlocked"));
    if let Some(attempt) = entry.reblank_attempt {
        lines.push(format!("Re-blank attempt: {attempt}"));
    }
    if let Some(method) = entry.blank_method {
        lines.push(format!("Blank method: {}", method.as_str()));
    }
    lines.push(overlay_line(entry));
    if let Some(line) = prompt_line(entry) {
        lines.push(line);
    }
    lines
}

fn row_label(entry: &HistoryEntry) -> String {
    let kind = kind_label(entry.kind);
    match (entry.from, entry.to) {
        (Some(from), Some(to)) => format!("{}  {kind}  {from} -> {to}", entry.at),
        _ => format!("{}  {kind}", entry.at),
    }
}

fn output_line(output: &OutputStats) -> String {
    format!(
        "{}: persistent {}, dark {}, counted {}",
        output.output,
        percent(output.persistent_percent),
        percent(output.dark_percent),
        percent(output.counted_percent),
    )
}

fn overlay_line(entry: &HistoryEntry) -> String {
    let used =
        entry.kind == HistoryKind::OverlayUsed || entry.blank_method == Some(BlankMethod::Overlay);
    format!("Overlay: {}", if used { "used" } else { "not used" })
}

fn prompt_line(entry: &HistoryEntry) -> Option<String> {
    match (entry.prompt_style, entry.prompt_reason) {
        (Some(style), Some(reason)) => {
            Some(format!("Prompt: {} ({})", style.as_str(), reason.as_str()))
        }
        (Some(style), None) => Some(format!("Prompt: {}", style.as_str())),
        (None, Some(reason)) => Some(format!("Prompt reason: {}", reason.as_str())),
        (None, None) => None,
    }
}

fn flag(name: &str, on: bool, yes: &str, no: &str) -> String {
    format!("{name}: {}", if on { yes } else { no })
}

fn percent(value: f64) -> String {
    if value.is_finite() {
        format!("{:.0}%", value.clamp(0.0, 100.0))
    } else {
        "0%".to_owned()
    }
}

const fn threshold_why(reason: ThresholdReason) -> &'static str {
    match reason {
        ThresholdReason::Normal => "normal",
        ThresholdReason::Media => "media",
        ThresholdReason::Ceiling => "ceiling",
    }
}
