//! The probe grid: one block grid per output with a summary line, and a
//! legend. Shared by `stillwatch probe` over D-Bus and the standalone probe.

use std::fmt::Write as _;

use stillwatch_core::stats::{BlockCounts, BlockState};
use stillwatch_ipc::probe::{ProbeOutput, ProbeSample};

use super::{Paint, Style, output_summary, verdict};

/// Clears the terminal and moves the cursor home, for redrawing in place.
pub const CLEAR: &str = "\x1b[H\x1b[2J";

const STATES: [BlockState; 4] = [
    BlockState::Persistent,
    BlockState::Changed,
    BlockState::Dark,
    BlockState::Ignored,
];

/// Renders one sample: a header with the time and the screen verdict, then
/// each output's summary line and grid, then the legend. Each block is two
/// characters wide so the grid keeps roughly the screen's proportions.
///
/// ```text
/// 2026-10-02 20:50:13  screen: STALE
///
/// HDMI-A-1: persistent 75% (dark 25%, counted 3/4), threshold 70% normal -> STALE
/// ████░░
/// ····xx
///
/// ██ persistent  ░░ changed  ·· dark  xx ignored
/// ```
#[must_use]
pub fn render(sample: &ProbeSample, style: &Style) -> String {
    let mut out = format!(
        "{}  screen: {}\n",
        style.time(sample.at),
        verdict(sample.stale, style)
    );
    if sample.outputs.is_empty() {
        out.push_str("\nno monitored outputs\n");
    }
    for output in &sample.outputs {
        out.push('\n');
        out.push_str(&output_line(output, sample, style));
        out.push('\n');
        out.push_str(&grid(output, style));
    }
    out.push('\n');
    out.push_str(&legend(style));
    out
}

fn output_line(output: &ProbeOutput, sample: &ProbeSample, style: &Style) -> String {
    let counts = BlockCounts::from_states(&output.blocks);
    let counted = format!("{}/{}", counts.counted, counts.total);
    output_summary(&output.stats, &counted, sample.threshold, style)
}

/// Rows of blocks. Runs of the same state are painted together to keep the
/// escape codes down.
fn grid(output: &ProbeOutput, style: &Style) -> String {
    let mut out = String::new();
    for row in output.blocks.chunks(usize::from(output.columns.max(1))) {
        let mut rest = row;
        while let Some(&state) = rest.first() {
            let run = rest.iter().take_while(|s| **s == state).count();
            out.push_str(&style.paint(&glyph(state).repeat(run), paint(state)));
            rest = &rest[run..];
        }
        out.push('\n');
    }
    out
}

fn legend(style: &Style) -> String {
    let mut out = String::new();
    for (i, state) in STATES.into_iter().enumerate() {
        let gap = if i == 0 { "" } else { "  " };
        let swatch = style.paint(glyph(state), paint(state));
        let _ = write!(out, "{gap}{swatch} {}", name(state));
    }
    out.push('\n');
    out
}

const fn glyph(state: BlockState) -> &'static str {
    match state {
        BlockState::Persistent => "██",
        BlockState::Changed => "░░",
        BlockState::Dark => "··",
        BlockState::Ignored => "xx",
    }
}

const fn paint(state: BlockState) -> Paint {
    match state {
        BlockState::Persistent => Paint::Red,
        BlockState::Changed => Paint::Green,
        BlockState::Dark => Paint::Dim,
        BlockState::Ignored => Paint::Blue,
    }
}

const fn name(state: BlockState) -> &'static str {
    match state {
        BlockState::Persistent => "persistent",
        BlockState::Changed => "changed",
        BlockState::Dark => "dark",
        BlockState::Ignored => "ignored",
    }
}

#[cfg(test)]
mod tests;
