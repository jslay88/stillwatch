//! Probe samples become heatmap cells. States and percentages only.

use stillwatch_core::stats::BlockCounts;
use stillwatch_ipc::probe::{ProbeOutput, ProbeSample};

use super::{OutputHeat, ProbeView};

/// The block under a point in a heatmap of `width` by `height` pixels.
///
/// Coordinates outside the heatmap clamp to the nearest block. `None` when
/// the grid or the widget has no area.
#[must_use]
pub fn cell_at(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    columns: u16,
    rows: u16,
) -> Option<(u16, u16)> {
    if columns == 0 || rows == 0 || width <= 0.0 || height <= 0.0 {
        return None;
    }
    if !x.is_finite() || !y.is_finite() {
        return None;
    }
    let x = x.clamp(0.0, width.next_down().max(0.0));
    let y = y.clamp(0.0, height.next_down().max(0.0));
    Some((bucket(x, width, columns), bucket(y, height, rows)))
}

fn bucket(pos: f32, span: f32, cells: u16) -> u16 {
    let last = cells.saturating_sub(1);
    let step = span / f32::from(cells);
    if step <= 0.0 {
        return 0;
    }
    let mut index = 0;
    while index < last && pos >= f32::from(index + 1) * step {
        index += 1;
    }
    index
}

/// Heatmaps for `sample`. An output whose block list doesn't match its grid
/// is left out.
#[must_use]
pub fn view_of(sample: &ProbeSample) -> ProbeView {
    ProbeView {
        threshold_percent: sample.threshold.percent,
        threshold_reason: sample.threshold.reason,
        stale: sample.stale,
        outputs: sample.outputs.iter().filter_map(output_heat).collect(),
    }
}

fn output_heat(output: &ProbeOutput) -> Option<OutputHeat> {
    let expected = usize::from(output.columns) * usize::from(output.rows);
    if output.columns == 0 || output.rows == 0 || output.blocks.len() != expected {
        return None;
    }
    let counts = BlockCounts::from_states(&output.blocks);
    Some(OutputHeat {
        name: output.stats.output.clone(),
        columns: output.columns,
        rows: output.rows,
        width: output.width,
        height: output.height,
        cells: output.blocks.clone(),
        persistent: counts.persistent,
        dark: counts.dark,
        counted: counts.counted,
        total: counts.total,
        persistent_percent: percent_text(output.stats.persistent_percent),
        dark_percent: percent_text(output.stats.dark_percent),
        stale: output.stats.stale,
    })
}

fn percent_text(value: f64) -> String {
    if !value.is_finite() {
        return "0%".to_owned();
    }
    format!("{:.0}%", value.clamp(0.0, 100.0))
}
