//! The block grid, drawn from states. No luma, no pixels.

use iced::widget::canvas::{self, Path, Stroke};
use iced::{Color, Element, Length, Point, Rectangle, Size, mouse};

use stillwatch_core::detector::{PixelSpan, pixels_to_blocks};
use stillwatch_core::stats::BlockState;

use crate::settings::RegionInput;
use crate::shell::Message;

use super::heat::cell_at;
use super::{CalMsg, Drag, OutputHeat};

const MAX_SIDE: f32 = 320.0;

/// Changed, persistent, dark, ignored. Same roles as the probe grid.
#[must_use]
pub fn swatch(state: BlockState) -> Color {
    match state {
        BlockState::Changed => Color::from_rgb8(0x3d, 0xd6, 0x8c),
        BlockState::Persistent => Color::from_rgb8(0xe0, 0x4f, 0x5f),
        BlockState::Dark => Color::from_rgb8(0x3a, 0x3a, 0x42),
        BlockState::Ignored => Color::from_rgb8(0x6c, 0x8e, 0xbf),
    }
}

/// Label next to a legend swatch.
#[must_use]
pub const fn state_name(state: BlockState) -> &'static str {
    match state {
        BlockState::Changed => "changed",
        BlockState::Persistent => "persistent",
        BlockState::Dark => "dark",
        BlockState::Ignored => "ignored",
    }
}

/// Legend order.
pub const LEGEND: [BlockState; 4] = [
    BlockState::Persistent,
    BlockState::Changed,
    BlockState::Dark,
    BlockState::Ignored,
];

/// One output's heatmap. Drags become [`CalMsg`]s.
#[must_use]
pub fn heatmap<'a>(
    output: &'a OutputHeat,
    drag: Option<&'a Drag>,
    regions: &'a [RegionInput],
) -> Element<'a, Message> {
    let (width, height) = heatmap_size(output.columns, output.rows);
    canvas::Canvas::new(Grid {
        output,
        drag,
        regions,
    })
    .width(Length::Fixed(width))
    .height(Length::Fixed(height))
    .into()
}

fn heatmap_size(columns: u16, rows: u16) -> (f32, f32) {
    let columns = f32::from(columns.max(1));
    let rows = f32::from(rows.max(1));
    let cell = (MAX_SIDE / columns).min(MAX_SIDE / rows).clamp(4.0, 28.0);
    (cell * columns, cell * rows)
}

struct Grid<'a> {
    output: &'a OutputHeat,
    drag: Option<&'a Drag>,
    regions: &'a [RegionInput],
}

impl canvas::Program<Message> for Grid<'_> {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let canvas::Event::Mouse(event) = event else {
            return None;
        };
        let message = match event {
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                let (column, row) = cell_in(bounds, cursor, self.output)?;
                CalMsg::BeginDrag {
                    output: self.output.name.clone(),
                    column,
                    row,
                }
            }
            mouse::Event::CursorMoved { .. } => {
                let drag = self.drag?;
                if drag.output != self.output.name {
                    return None;
                }
                let (column, row) = cell_in(bounds, cursor, self.output)?;
                CalMsg::MoveDrag { column, row }
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) => {
                let drag = self.drag?;
                if drag.output != self.output.name {
                    return None;
                }
                CalMsg::FinishDrag
            }
            _ => return None,
        };
        Some(canvas::Action::publish(Message::Calibration(message)).and_capture())
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        fill_cells(&mut frame, self.output);
        stroke_regions(&mut frame, self.output, self.regions);
        if let Some(drag) = self.drag.filter(|drag| drag.output == self.output.name) {
            stroke_drag(&mut frame, self.output, drag);
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        mouse::Interaction::Crosshair
    }
}

fn cell_in(bounds: Rectangle, cursor: mouse::Cursor, output: &OutputHeat) -> Option<(u16, u16)> {
    let point = cursor.position_in(bounds)?;
    cell_at(
        point.x,
        point.y,
        bounds.width,
        bounds.height,
        output.columns,
        output.rows,
    )
}

fn fill_cells(frame: &mut canvas::Frame, output: &OutputHeat) {
    let columns = output.columns.max(1);
    let cell_w = frame.width() / f32::from(columns);
    let cell_h = frame.height() / f32::from(output.rows.max(1));
    for row in 0..output.rows {
        for column in 0..columns {
            let index = usize::from(row) * usize::from(columns) + usize::from(column);
            let Some(state) = output.cells.get(index) else {
                continue;
            };
            let path = Path::rectangle(
                Point::new(f32::from(column) * cell_w, f32::from(row) * cell_h),
                Size::new(cell_w, cell_h),
            );
            frame.fill(&path, swatch(*state));
        }
    }
}

fn stroke_regions(frame: &mut canvas::Frame, output: &OutputHeat, regions: &[RegionInput]) {
    for region in regions.iter().filter(|region| region.output == output.name) {
        let Some(span) = span_of(region) else {
            continue;
        };
        let Some(blocks) = pixels_to_blocks(
            span,
            output.width,
            output.height,
            u32::from(output.columns),
            u32::from(output.rows),
        ) else {
            continue;
        };
        stroke_blocks(
            frame,
            output,
            blocks.column,
            blocks.row,
            blocks.columns,
            blocks.rows,
            Color::WHITE,
        );
    }
}

fn span_of(region: &RegionInput) -> Option<PixelSpan> {
    Some(PixelSpan {
        x: region.x.trim().parse().ok()?,
        y: region.y.trim().parse().ok()?,
        w: region.w.trim().parse().ok()?,
        h: region.h.trim().parse().ok()?,
    })
}

fn unit(value: u32) -> f32 {
    f32::from(u16::try_from(value).unwrap_or(u16::MAX))
}

fn stroke_drag(frame: &mut canvas::Frame, output: &OutputHeat, drag: &Drag) {
    let column = u32::from(drag.start_column.min(drag.column));
    let row = u32::from(drag.start_row.min(drag.row));
    let columns = u32::from(drag.start_column.abs_diff(drag.column)) + 1;
    let rows = u32::from(drag.start_row.abs_diff(drag.row)) + 1;
    stroke_blocks(
        frame,
        output,
        column,
        row,
        columns,
        rows,
        Color::from_rgb8(0xf5, 0xd0, 0x6a),
    );
}

fn stroke_blocks(
    frame: &mut canvas::Frame,
    output: &OutputHeat,
    column: u32,
    row: u32,
    columns: u32,
    rows: u32,
    color: Color,
) {
    let grid_cols = f32::from(output.columns.max(1));
    let grid_rows = f32::from(output.rows.max(1));
    let cell_w = frame.width() / grid_cols;
    let cell_h = frame.height() / grid_rows;
    let path = Path::rectangle(
        Point::new(unit(column) * cell_w, unit(row) * cell_h),
        Size::new(unit(columns) * cell_w, unit(rows) * cell_h),
    );
    frame.stroke(&path, Stroke::default().with_width(2.0).with_color(color));
}
