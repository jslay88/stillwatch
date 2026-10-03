use proptest::collection::vec;
use proptest::option;
use proptest::prelude::*;
use proptest::sample::select;

use super::*;
use crate::config::{IgnoreRegion, StaleRequire};
use crate::luma::LumaGrid;
use crate::stats::BlockCounts;

const SIZE: u32 = 400;
const PALETTE: [f32; 7] = [0.0, 5.0, 12.0, 15.9, 100.0, 103.0, 200.0];

type Rect = (u32, u32, u32, u32);

fn grid_and_captures() -> impl Strategy<Value = (u32, u32, Vec<Vec<f32>>)> {
    (1..6u32, 1..6u32).prop_flat_map(|(cols, rows)| {
        let blocks = (cols * rows) as usize;
        (
            Just(cols),
            Just(rows),
            vec(vec(select(&PALETTE[..]), blocks), 1..12),
        )
    })
}

/// Whether the real-valued rect of block `index` overlaps `region`.
fn overlaps(region: Option<Rect>, index: usize, cols: u32, rows: u32) -> bool {
    let Some((x, y, w, h)) = region else {
        return false;
    };
    let index = u32::try_from(index).unwrap();
    let (col, row) = (index % cols, index / cols);
    let span = |cell: u32, cells: u32| {
        let size = f64::from(SIZE) / f64::from(cells);
        (f64::from(cell) * size, f64::from(cell + 1) * size)
    };
    let ((left, right), (top, bottom)) = (span(col, cols), span(row, rows));
    f64::from(x) < right
        && f64::from(x + w) > left
        && f64::from(y) < bottom
        && f64::from(y + h) > top
}

fn assert_percentages(stats: &DetectionStats) {
    for output in &stats.outputs {
        assert!((0.0..=100.0).contains(&output.persistent_percent));
        assert!(output.counted_percent <= 100.0);
        assert!(output.dark_percent + output.counted_percent <= 100.0 + 1e-9);
    }
}

proptest! {
    #[test]
    fn block_states_respect_the_algorithm_invariants(
        (cols, rows, captures) in grid_and_captures(),
        persist_checks in 1..4u32,
        dark_below in select(vec![0u8, 16]),
        region in option::of((0..SIZE, 0..SIZE, 1..SIZE, 1..SIZE)),
    ) {
        let mut config = Config::default();
        config.stale.block_grid = [cols, rows];
        config.stale.persist_checks = persist_checks;
        config.stale.ignore_dark_below = u32::from(dark_below);
        config.stale.ignore_regions = region
            .map(|(x, y, w, h)| IgnoreRegion { output: "DP-1".into(), x, y, w, h })
            .into_iter()
            .collect();
        let mut detector = BlockDetector::new(&config);
        detector.set_outputs(&[OutputInfo::new("DP-1", SIZE, SIZE)]);
        let dark = |mean: f32| mean < f32::from(dark_below);

        let mut previous: Option<&Vec<f32>> = None;
        for means in &captures {
            let stats = detector.observe_means(&[BlockMeans { output: "DP-1", means }], &[]);
            assert_percentages(&stats);
            assert_percentages(&detector.ceiling().unwrap_or(stats.clone()));
            let blocks = detector.blocks("DP-1").unwrap();
            let counts = BlockCounts::from_states(blocks);
            prop_assert_eq!(counts.total as usize, means.len());
            prop_assert!(counts.counted <= counts.total);
            for (index, &state) in blocks.iter().enumerate() {
                let ignored = overlaps(region, index, cols, rows);
                let was_dark = previous.is_some_and(|prev| dark(prev[index]) && dark(means[index]));
                if ignored {
                    prop_assert_eq!(state, BlockState::Ignored);
                } else if was_dark {
                    prop_assert_eq!(state, BlockState::Dark);
                } else {
                    prop_assert!(matches!(state, BlockState::Changed | BlockState::Persistent));
                }
            }
            if counts.counted == 0 {
                prop_assert!(!stats.stale);
            }
            previous = Some(means);
        }
    }

    #[test]
    fn constant_image_is_stale_after_exactly_persist_checks_unchanged_captures(
        value in 16..=255u8,
        persist_checks in 1..8u32,
        cols in 1..5u32,
        rows in 1..5u32,
    ) {
        let mut config = Config::default();
        config.stale.block_grid = [cols, rows];
        config.stale.persist_checks = persist_checks;
        let mut detector = BlockDetector::new(&config);
        let means = vec![f32::from(value); (cols * rows) as usize];
        let frame = [BlockMeans { output: "DP-1", means: &means }];
        for _ in 0..persist_checks {
            prop_assert!(!detector.observe_means(&frame, &[]).stale);
        }
        prop_assert!(detector.observe_means(&frame, &[]).stale);
    }

    #[test]
    fn a_grid_that_does_not_fit_never_makes_the_screen_stale(
        (width, height) in (1..12u32, 1..12u32),
        (cols, rows) in (1..8u32, 1..8u32),
        value in 16..=255u8,
        persist_checks in 1..4u32,
        require in select(vec![StaleRequire::All, StaleRequire::Any]),
    ) {
        let mut config = Config::default();
        config.stale.block_grid = [cols, rows];
        config.stale.persist_checks = persist_checks;
        config.stale.require = require;
        config.safety.ceiling_minutes = persist_checks;
        let mut detector = BlockDetector::new(&config);
        let frames = [
            CaptureFrame { output: "DP-1".into(), grid: LumaGrid::filled(8, 8, value).unwrap() },
            CaptureFrame { output: "DP-2".into(), grid: LumaGrid::filled(width, height, value).unwrap() },
        ];
        let fits = cols <= width && rows <= height;
        for _ in 0..persist_checks {
            prop_assert!(!detector.observe(&frames, &[]).stale);
        }
        let stats = detector.observe(&frames, &[]);
        let expected = fits || require == StaleRequire::Any;
        prop_assert_eq!(stats.stale, expected);
        prop_assert_eq!(detector.ceiling().unwrap().stale, expected);
        prop_assert_eq!(stats.outputs.len(), 2);
        prop_assert_eq!(stats.outputs[1].stale, fits);
        prop_assert_eq!(detector.blocks("DP-2").is_some(), fits);
    }
}
