//! Drags and region buttons become settings edits.

use stillwatch_core::detector::{BlockRect, blocks_to_pixels};

use crate::edit_msg::FieldChange;

use super::{CalMsg, Calibration, Drag};

const IGNORE_KEY: &str = "stale.ignore_regions";

/// Applies `message` to the calibration state.
///
/// `region_count` is how many ignore regions the form currently has. A
/// returned edit goes through the settings form; nothing is written until Save.
#[must_use]
pub fn handle(cal: &mut Calibration, region_count: usize, message: CalMsg) -> Option<FieldChange> {
    match message {
        CalMsg::Pace(pace) => {
            cal.pace = pace;
            None
        }
        CalMsg::Slider { key, value } => Some(FieldChange::Text {
            key,
            value: value.to_string(),
        }),
        CalMsg::BeginDrag {
            output,
            column,
            row,
        } => {
            cal.place_error = None;
            cal.drag = Some(Drag {
                output,
                start_column: column,
                start_row: row,
                column,
                row,
            });
            None
        }
        CalMsg::MoveDrag { column, row } => {
            if let Some(drag) = cal.drag.as_mut() {
                drag.column = column;
                drag.row = row;
            }
            None
        }
        CalMsg::FinishDrag => finish(cal, region_count),
        CalMsg::Edit(index) => {
            if index < region_count {
                cal.editing = Some(index);
                cal.place_error = None;
            }
            None
        }
        CalMsg::Delete(index) => delete(cal, region_count, index),
    }
}

fn delete(cal: &mut Calibration, region_count: usize, index: usize) -> Option<FieldChange> {
    if index >= region_count {
        return None;
    }
    cal.editing = match cal.editing {
        Some(current) if current == index => None,
        Some(current) if current > index => Some(current - 1),
        other => other,
    };
    Some(FieldChange::RegionRemove {
        key: IGNORE_KEY.to_owned(),
        index,
    })
}

fn finish(cal: &mut Calibration, region_count: usize) -> Option<FieldChange> {
    let drag = cal.drag.take()?;
    let heat = cal.view.as_ref().and_then(|view| {
        view.outputs
            .iter()
            .find(|item| item.name == drag.output)
            .map(|item| (item.columns, item.rows, item.width, item.height))
    });
    let Some((columns, rows, width, height)) = heat else {
        cal.place_error = Some(format!("No heatmap for {}.", drag.output));
        return None;
    };
    if width == 0 || height == 0 {
        cal.place_error = Some(format!(
            "{} has no reported size, so a region can't be placed in output pixels.",
            drag.output
        ));
        return None;
    }
    let blocks = block_rect(&drag, columns, rows)?;
    let Some(pixels) = blocks_to_pixels(blocks, width, height, u32::from(columns), u32::from(rows))
    else {
        cal.place_error = Some(format!("Couldn't map that rectangle onto {}.", drag.output));
        return None;
    };
    let index = cal.editing.take().filter(|index| *index < region_count);
    Some(FieldChange::RegionSet {
        key: IGNORE_KEY.to_owned(),
        index,
        output: drag.output,
        x: pixels.x.to_string(),
        y: pixels.y.to_string(),
        w: pixels.w.to_string(),
        h: pixels.h.to_string(),
    })
}

fn block_rect(drag: &Drag, columns: u16, rows: u16) -> Option<BlockRect> {
    if columns == 0 || rows == 0 {
        return None;
    }
    let start_column = drag.start_column.min(columns - 1);
    let start_row = drag.start_row.min(rows - 1);
    let column = drag.column.min(columns - 1);
    let row = drag.row.min(rows - 1);
    Some(BlockRect {
        column: u32::from(start_column.min(column)),
        row: u32::from(start_row.min(row)),
        columns: u32::from(start_column.abs_diff(column)) + 1,
        rows: u32::from(start_row.abs_diff(row)) + 1,
    })
}
