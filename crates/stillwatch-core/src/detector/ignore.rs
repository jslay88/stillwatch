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
