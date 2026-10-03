//! Maps `ignore_regions` (pixel rects in output coordinates) to grid blocks.
//!
//! The downscaled luma grid covers the whole output, so block `(c, r)` of a
//! `cols x rows` grid covers output pixels `[c * W / cols, (c + 1) * W / cols)`
//! horizontally and `[r * H / rows, (r + 1) * H / rows)` vertically, where `W x H`
//! is the output's size from [`OutputInfo`]. The bounds are compared exactly
//! (no rounding), and a region that overlaps any part of a block ignores the
//! whole block.

use std::ops::Range;

use crate::config::IgnoreRegion;
use crate::luma::OutputInfo;

/// Row-major mask of ignored blocks for `output`. Empty when no region applies.
pub(crate) fn ignore_mask(
    regions: &[IgnoreRegion],
    output: &OutputInfo,
    [cols, rows]: [u32; 2],
) -> Vec<bool> {
    let mut mask = Vec::new();
    for region in regions.iter().filter(|region| region.output == output.name) {
        let columns = overlapped(region.x, region.w, output.width, cols);
        let block_rows = overlapped(region.y, region.h, output.height, rows);
        if columns.is_empty() || block_rows.is_empty() {
            continue;
        }
        if mask.is_empty() {
            mask = vec![false; cols as usize * rows as usize];
        }
        for row in block_rows {
            let start = row as usize * cols as usize;
            for cell in &mut mask[start + columns.start as usize..start + columns.end as usize] {
                *cell = true;
            }
        }
    }
    mask
}

/// A rectangle of blocks on one output's grid.
///
/// `column` and `row` are the top-left block. `columns` and `rows` are how
/// many blocks the rectangle covers, at least 1 when it came from a drag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockRect {
    /// Leftmost block.
    pub column: u32,
    /// Topmost block.
    pub row: u32,
    /// How many columns the rectangle covers.
    pub columns: u32,
    /// How many rows the rectangle covers.
    pub rows: u32,
}

/// A rectangle in output pixels, the shape of an `ignore_regions` entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelSpan {
    /// Left edge.
    pub x: u32,
    /// Top edge.
    pub y: u32,
    /// Width. Zero covers nothing.
    pub w: u32,
    /// Height. Zero covers nothing.
    pub h: u32,
}

/// Blocks of a `cols` by `rows` grid that `span` overlaps on an output of
/// `width` by `height` pixels.
///
/// The same rule as ignore regions: a block is included when the rectangle
/// touches any of its pixels, including when `width` doesn't divide `cols`.
/// `None` when the rectangle misses the output or the grid is empty.
#[must_use]
pub fn pixels_to_blocks(
    span: PixelSpan,
    width: u32,
    height: u32,
    cols: u32,
    rows: u32,
) -> Option<BlockRect> {
    let columns = overlapped(span.x, span.w, width, cols);
    let block_rows = overlapped(span.y, span.h, height, rows);
    rect_from_ranges(columns, block_rows)
}

/// Output pixels that overlap exactly `blocks` and no other block.
///
/// Drawing on the grid snaps to block boundaries. The returned span is the
/// one [`pixels_to_blocks`] maps back to `blocks`, using the same integer
/// bounds as ignore regions, so a size that doesn't divide the grid still
/// lands on the blocks that were drawn.
#[must_use]
pub fn blocks_to_pixels(
    blocks: BlockRect,
    width: u32,
    height: u32,
    cols: u32,
    rows: u32,
) -> Option<PixelSpan> {
    let (x, w) = axis_pixels(blocks.column, blocks.columns, width, cols)?;
    let (y, h) = axis_pixels(blocks.row, blocks.rows, height, rows)?;
    let span = PixelSpan { x, y, w, h };
    (pixels_to_blocks(span, width, height, cols, rows) == Some(blocks)).then_some(span)
}

fn rect_from_ranges(columns: Range<u32>, rows: Range<u32>) -> Option<BlockRect> {
    if columns.is_empty() || rows.is_empty() {
        return None;
    }
    Some(BlockRect {
        column: columns.start,
        row: rows.start,
        columns: columns.end.saturating_sub(columns.start),
        rows: rows.end.saturating_sub(rows.start),
    })
}

/// Pixel `[start, start + len)` that overlaps blocks `[first, past)` only.
fn axis_pixels(first: u32, count: u32, size: u32, cells: u32) -> Option<(u32, u32)> {
    let past = first.checked_add(count)?;
    if count == 0 || cells == 0 || size == 0 || past > cells {
        return None;
    }
    let size_wide = u64::from(size);
    let cells_wide = u64::from(cells);
    let first_wide = u64::from(first);
    let past_wide = u64::from(past);
    let start = div_ceil(first_wide * size_wide, cells_wide);
    let end = past_wide * size_wide / cells_wide;
    if start >= end {
        return None;
    }
    let start = u32::try_from(start).ok()?;
    let len = u32::try_from(end - u64::from(start)).ok()?;
    let covered = overlapped(start, len, size, cells);
    (covered == (first..past)).then_some((start, len))
}

fn div_ceil(num: u64, den: u64) -> u64 {
    num.div_ceil(den)
}

/// Indices of the `cells` equal slices of `size` pixels that `[start, start + len)` touches.
fn overlapped(start: u32, len: u32, size: u32, cells: u32) -> Range<u32> {
    if size == 0 || len == 0 {
        return 0..0;
    }
    let (size, cells_wide) = (u64::from(size), u64::from(cells));
    let end = u64::from(start) + u64::from(len);
    let first = (u64::from(start) * cells_wide / size).min(cells_wide);
    let past_last = (end * cells_wide).div_ceil(size).clamp(first, cells_wide);
    let narrow = |value: u64| u32::try_from(value).unwrap_or(cells);
    narrow(first)..narrow(past_last)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTHING: [bool; 0] = [];

    fn region(output: &str, x: u32, y: u32, w: u32, h: u32) -> IgnoreRegion {
        IgnoreRegion {
            output: output.to_owned(),
            x,
            y,
            w,
            h,
        }
    }

    fn ignored_cells(mask: &[bool], cols: usize) -> Vec<(usize, usize)> {
        mask.iter()
            .enumerate()
            .filter(|(_, ignored)| **ignored)
            .map(|(index, _)| (index % cols, index / cols))
            .collect()
    }

    #[test]
    fn region_inside_one_block_ignores_only_that_block() {
        let output = OutputInfo::new("HDMI-A-1", 3840, 2160);
        let mask = ignore_mask(&[region("HDMI-A-1", 0, 0, 200, 40)], &output, [16, 16]);
        assert_eq!(ignored_cells(&mask, 16), vec![(0, 0)]);
    }

    #[test]
    fn region_straddling_block_boundaries_ignores_every_block_it_touches() {
        let output = OutputInfo::new("DP-1", 400, 400);
        let mask = ignore_mask(&[region("DP-1", 99, 99, 2, 2)], &output, [4, 4]);
        assert_eq!(
            ignored_cells(&mask, 4),
            vec![(0, 0), (1, 0), (0, 1), (1, 1)]
        );
    }

    #[test]
    fn region_ending_on_a_boundary_does_not_spill_over() {
        let output = OutputInfo::new("DP-1", 400, 400);
        let mask = ignore_mask(&[region("DP-1", 100, 0, 100, 100)], &output, [4, 4]);
        assert_eq!(ignored_cells(&mask, 4), vec![(1, 0)]);
    }

    #[test]
    fn uneven_block_sizes_use_exact_bounds() {
        let output = OutputInfo::new("DP-1", 10, 10);
        let mask = ignore_mask(&[region("DP-1", 2, 0, 1, 1)], &output, [3, 1]);
        assert_eq!(ignored_cells(&mask, 3), vec![(0, 0)]);
        let mask = ignore_mask(&[region("DP-1", 3, 0, 1, 1)], &output, [3, 1]);
        assert_eq!(ignored_cells(&mask, 3), vec![(0, 0), (1, 0)]);
        let mask = ignore_mask(&[region("DP-1", 4, 0, 2, 1)], &output, [3, 1]);
        assert_eq!(ignored_cells(&mask, 3), vec![(1, 0)]);
    }

    #[test]
    fn regions_past_the_edge_are_clamped() {
        let output = OutputInfo::new("DP-1", 400, 400);
        let mask = ignore_mask(&[region("DP-1", 350, 390, 500, 500)], &output, [4, 4]);
        assert_eq!(ignored_cells(&mask, 4), vec![(3, 3)]);
        let outside = ignore_mask(&[region("DP-1", 400, 0, 10, 10)], &output, [4, 4]);
        assert_eq!(outside, NOTHING);
    }

    #[test]
    fn other_outputs_and_empty_sizes_ignore_nothing() {
        let output = OutputInfo::new("DP-1", 400, 400);
        let other_output = ignore_mask(&[region("DP-2", 0, 0, 400, 400)], &output, [4, 4]);
        assert_eq!(other_output, NOTHING);
        let unknown_size = OutputInfo::new("DP-1", 0, 0);
        let mask = ignore_mask(&[region("DP-1", 0, 0, 400, 400)], &unknown_size, [4, 4]);
        assert_eq!(mask, NOTHING);
        let zero_width = ignore_mask(&[region("DP-1", 0, 0, 0, 10)], &output, [4, 4]);
        assert_eq!(zero_width, NOTHING);
    }

    #[test]
    fn uneven_grids_round_trip_a_drawn_block_rectangle() {
        let cases = [(10, 10, 3, 1), (1000, 640, 6, 5), (3840, 2160, 16, 16)];
        for (width, height, cols, rows) in cases {
            for column in 0..cols {
                for row in 0..rows {
                    for columns in 1..=(cols - column) {
                        for block_rows in 1..=(rows - row) {
                            let blocks = BlockRect {
                                column,
                                row,
                                columns,
                                rows: block_rows,
                            };
                            let pixels = blocks_to_pixels(blocks, width, height, cols, rows)
                                .unwrap_or_else(|| {
                                    panic!("no pixels for {blocks:?} on {width}x{height}")
                                });
                            assert_eq!(
                                pixels_to_blocks(pixels, width, height, cols, rows),
                                Some(blocks),
                                "{pixels:?} on {width}x{height} grid {cols}x{rows}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_straddling_pixel_rect_expands_to_every_block_it_touches() {
        let pixels = PixelSpan {
            x: 99,
            y: 99,
            w: 2,
            h: 2,
        };
        assert_eq!(
            pixels_to_blocks(pixels, 400, 400, 4, 4),
            Some(BlockRect {
                column: 0,
                row: 0,
                columns: 2,
                rows: 2
            })
        );
        let snapped = blocks_to_pixels(
            BlockRect {
                column: 0,
                row: 0,
                columns: 2,
                rows: 2,
            },
            400,
            400,
            4,
            4,
        )
        .unwrap();
        assert_eq!(
            snapped,
            PixelSpan {
                x: 0,
                y: 0,
                w: 200,
                h: 200
            }
        );
    }

    #[test]
    fn empty_and_out_of_range_rects_are_rejected() {
        let blocks = BlockRect {
            column: 0,
            row: 0,
            columns: 1,
            rows: 1,
        };
        assert_eq!(blocks_to_pixels(blocks, 0, 10, 3, 1), None);
        assert_eq!(blocks_to_pixels(blocks, 10, 10, 0, 1), None);
        assert_eq!(
            blocks_to_pixels(
                BlockRect {
                    column: 2,
                    row: 0,
                    columns: 2,
                    rows: 1
                },
                10,
                10,
                3,
                1
            ),
            None
        );
        assert_eq!(
            pixels_to_blocks(
                PixelSpan {
                    x: 0,
                    y: 0,
                    w: 0,
                    h: 1
                },
                10,
                10,
                3,
                1
            ),
            None
        );
        assert_eq!(
            pixels_to_blocks(
                PixelSpan {
                    x: 10,
                    y: 0,
                    w: 1,
                    h: 1
                },
                10,
                10,
                3,
                1
            ),
            None
        );
    }

    #[test]
    fn drawn_blocks_match_the_ignore_mask() {
        let output = OutputInfo::new("DP-1", 10, 10);
        let blocks = BlockRect {
            column: 1,
            row: 0,
            columns: 1,
            rows: 1,
        };
        let pixels = blocks_to_pixels(blocks, 10, 10, 3, 1).unwrap();
        let mask = ignore_mask(
            &[region("DP-1", pixels.x, pixels.y, pixels.w, pixels.h)],
            &output,
            [3, 1],
        );
        assert_eq!(ignored_cells(&mask, 3), vec![(1, 0)]);
    }

    #[test]
    fn several_regions_combine() {
        let output = OutputInfo::new("DP-1", 400, 400);
        let regions = [
            region("DP-1", 0, 0, 10, 10),
            region("DP-1", 390, 390, 10, 10),
        ];
        let mask = ignore_mask(&regions, &output, [4, 4]);
        assert_eq!(ignored_cells(&mask, 4), vec![(0, 0), (3, 3)]);
    }
}
