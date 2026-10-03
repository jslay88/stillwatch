use proptest::prelude::*;

use super::*;

#[test]
fn divisible_grids_average_equal_blocks() {
    let grid = LumaGrid::from_fn(4, 2, |x, _| if x < 2 { 10 } else { 200 }).unwrap();
    let blocks = block_means(&grid, 2, 1).unwrap();
    assert_eq!((blocks.cols(), blocks.rows()), (2, 1));
    assert_eq!(blocks.means(), &[10.0, 200.0]);
}

#[test]
fn non_divisible_grids_assign_cells_by_floor() {
    let grid = LumaGrid::from_fn(5, 3, |x, y| u8::try_from(y * 5 + x).unwrap()).unwrap();
    let blocks = block_means(&grid, 2, 2).unwrap();
    // Columns split [0, 2) and [2, 5); rows split [0, 1) and [1, 3).
    assert_eq!(blocks.means(), &[0.5, 3.0, 8.0, 10.5]);
    assert_eq!(blocks.get(1, 1), Some(10.5));
    assert_eq!(blocks.get(2, 0), None);
    assert_eq!(blocks.get(0, 2), None);
}

#[test]
fn the_default_grid_fits_the_default_downscale() {
    let grid = LumaGrid::filled(480, 270, 77).unwrap();
    let blocks = block_means(&grid, 16, 16).unwrap();
    assert_eq!(blocks.means().len(), 256);
    assert!(
        blocks
            .means()
            .iter()
            .all(|&mean| mean.to_bits() == 77.0_f32.to_bits())
    );
}

#[test]
fn block_grids_that_dont_fit_error() {
    let grid = LumaGrid::filled(4, 3, 0).unwrap();
    for (cols, rows) in [(0, 1), (1, 0), (5, 1), (1, 4)] {
        assert_eq!(
            block_means(&grid, cols, rows),
            Err(BlockGridError {
                cols,
                rows,
                width: 4,
                height: 3
            })
        );
    }
    let err = block_means(&grid, 5, 1).unwrap_err();
    assert_eq!(
        err.to_string(),
        "a 5x1 block grid doesn't fit a 4x3 luma grid"
    );
}

proptest! {
    #[test]
    fn means_are_bounded_and_counted(
        width in 1_u32..64,
        height in 1_u32..64,
        cols in 1_u32..20,
        rows in 1_u32..20,
        seed in any::<u32>(),
    ) {
        let cols = cols.min(width);
        let rows = rows.min(height);
        let grid = LumaGrid::from_fn(width, height, |x, y| {
            u8::try_from(((x * 31 + y * 17) ^ seed) & 0xff).unwrap()
        })
        .unwrap();
        let blocks = block_means(&grid, cols, rows).unwrap();
        prop_assert_eq!(blocks.means().len(), cols as usize * rows as usize);
        let min = f32::from(*grid.data().iter().min().unwrap());
        let max = f32::from(*grid.data().iter().max().unwrap());
        prop_assert!(blocks.means().iter().all(|&mean| (min..=max).contains(&mean)));
        prop_assert!(blocks.means().iter().all(|&mean| (0.0..=255.0).contains(&mean)));
    }

    #[test]
    fn uniform_grids_give_uniform_means(
        width in 1_u32..64,
        height in 1_u32..64,
        cols in 1_u32..20,
        rows in 1_u32..20,
        level in any::<u8>(),
    ) {
        let grid = LumaGrid::filled(width, height, level).unwrap();
        let blocks = block_means(&grid, cols.min(width), rows.min(height)).unwrap();
        prop_assert!(blocks.means().iter().all(|&mean| mean.to_bits() == f32::from(level).to_bits()));
    }
}
