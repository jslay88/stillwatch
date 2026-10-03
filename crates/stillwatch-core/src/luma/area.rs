//! Area averaging: partitioning an image into boxes and summing each box.

/// Rows summed into the `u32` column sums before they are flushed:
/// `65536 * u16::MAX` still fits in a `u32`.
const FLUSH_ROWS: usize = 1 << 16;

/// Splits `total` cells into `parts` runs that cover them exactly. Run `i`
/// starts at `floor(i * total / parts)`, so runs differ in length by at most
/// one and none is empty while `parts <= total`.
pub(super) fn split(total: u32, parts: u32) -> Vec<usize> {
    let edge = |i: u32| {
        let edge = u64::from(i) * u64::from(total) / u64::from(parts.max(1));
        u32::try_from(edge).unwrap_or(total) as usize
    };
    (0..parts).map(|i| edge(i + 1) - edge(i)).collect()
}

/// Sums `cell` over each box of the grid formed by column runs `widths` and
/// row runs `heights`, calling `emit(sum, cell_count)` for every box in
/// row-major order.
///
/// Rows are streamed once, top to bottom. Each band of rows is summed per
/// column first and reduced to boxes at the end of the band, which keeps the
/// per-cell loop a straight `u32` add the compiler can vectorize. Only one
/// row of column sums is held at a time.
pub(super) fn sum_boxes<'a, T: 'a>(
    mut rows: impl Iterator<Item = &'a [T]>,
    widths: &[usize],
    heights: &[usize],
    cell: impl Fn(&T) -> u16,
    mut emit: impl FnMut(u64, usize),
) {
    let mut columns = vec![0_u32; widths.iter().sum()];
    let mut boxes = vec![0_u64; widths.len()];
    for &height in heights {
        boxes.fill(0);
        let mut left = height;
        while left > 0 {
            let take = left.min(FLUSH_ROWS);
            columns.fill(0);
            for row in rows.by_ref().take(take) {
                for (sum, value) in columns.iter_mut().zip(row) {
                    *sum += u32::from(cell(value));
                }
            }
            add_runs(&columns, widths, &mut boxes);
            left -= take;
        }
        for (&sum, &width) in boxes.iter().zip(widths) {
            emit(sum, width * height);
        }
    }
}

/// Adds the sum of each run of `columns` (lengths `widths`) to `boxes`.
fn add_runs(columns: &[u32], widths: &[usize], boxes: &mut [u64]) {
    let mut rest = columns;
    for (total, &width) in boxes.iter_mut().zip(widths) {
        let Some((span, tail)) = rest.split_at_checked(width) else {
            break;
        };
        *total += span.iter().map(|&sum| u64::from(sum)).sum::<u64>();
        rest = tail;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_covers_the_total_with_near_equal_runs() {
        assert_eq!(split(10, 3), vec![3, 3, 4]);
        assert_eq!(split(7, 7), vec![1; 7]);
        assert_eq!(split(3840, 480), vec![8; 480]);
        assert_eq!(split(5, 0), Vec::<usize>::new());
        let runs = split(1001, 16);
        assert_eq!(runs.iter().sum::<usize>(), 1001);
        assert!(runs.iter().all(|&run| run == 62 || run == 63));
    }

    #[test]
    fn sums_each_box_in_row_major_order() {
        let grid: [&[u8]; 3] = [&[1, 2, 3], &[4, 5, 6], &[7, 8, 9]];
        let mut boxes = Vec::new();
        sum_boxes(
            grid.into_iter(),
            &[1, 2],
            &[2, 1],
            |&v| u16::from(v),
            |sum, count| boxes.push((sum, count)),
        );
        assert_eq!(boxes, vec![(5, 2), (16, 4), (7, 1), (17, 2)]);
    }

    #[test]
    fn tall_bands_flush_before_the_column_sums_overflow() {
        let row: &[u16] = &[u16::MAX, 1];
        let height = FLUSH_ROWS * 2 + 3;
        let mut boxes = Vec::new();
        sum_boxes(
            std::iter::repeat_n(row, height),
            &[1, 1],
            &[height],
            |&v| v,
            |sum, count| boxes.push((sum, count)),
        );
        let height = height as u64;
        assert_eq!(
            boxes,
            vec![
                (height * u64::from(u16::MAX), FLUSH_ROWS * 2 + 3),
                (height, FLUSH_ROWS * 2 + 3)
            ]
        );
    }
}
