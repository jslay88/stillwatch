//! Per-block mean luma over a [`LumaGrid`].

use super::LumaGrid;
use super::area::{split, sum_boxes};

/// Mean luma of each block when a [`LumaGrid`] is divided into
/// `cols` x `rows` blocks.
///
/// Blocks cover the grid exactly. When the grid size isn't a multiple of the
/// block count, block `i` starts at cell `floor(i * width / cols)` (likewise
/// for rows), so block sizes differ by at most one cell.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockMeans {
    cols: u32,
    rows: u32,
    means: Vec<f32>,
}

impl BlockMeans {
    /// Blocks per row.
    #[must_use]
    pub const fn cols(&self) -> u32 {
        self.cols
    }

    /// Blocks per column.
    #[must_use]
    pub const fn rows(&self) -> u32 {
        self.rows
    }

    /// Row-major block means, each in `[0, 255]`.
    #[must_use]
    pub fn means(&self) -> &[f32] {
        &self.means
    }

    /// The mean of block `(col, row)`, or `None` when out of bounds.
    #[must_use]
    pub fn get(&self, col: u32, row: u32) -> Option<f32> {
        if col >= self.cols || row >= self.rows {
            return None;
        }
        self.means
            .get(row as usize * self.cols as usize + col as usize)
            .copied()
    }
}

/// Divides `grid` into `cols` x `rows` blocks and returns each block's mean
/// luma.
///
/// # Errors
///
/// Returns [`BlockGridError`] when `cols` or `rows` is zero or larger than
/// the grid, which would leave blocks with no cells.
pub fn block_means(grid: &LumaGrid, cols: u32, rows: u32) -> Result<BlockMeans, BlockGridError> {
    if cols == 0 || rows == 0 || cols > grid.width() || rows > grid.height() {
        return Err(BlockGridError {
            cols,
            rows,
            width: grid.width(),
            height: grid.height(),
        });
    }
    let mut means = Vec::with_capacity(cols as usize * rows as usize);
    sum_boxes(
        grid.data().chunks_exact(grid.width() as usize),
        &split(grid.width(), cols),
        &split(grid.height(), rows),
        |&luma| u16::from(luma),
        |sum, count| means.push(mean(sum, count)),
    );
    Ok(BlockMeans { cols, rows, means })
}

#[expect(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    reason = "a mean in [0, 255] keeps 7 significant digits in f32"
)]
fn mean(sum: u64, count: usize) -> f32 {
    (sum as f64 / count.max(1) as f64) as f32
}

/// A block grid that doesn't fit the luma grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
#[error("a {cols}x{rows} block grid doesn't fit a {width}x{height} luma grid")]
pub struct BlockGridError {
    /// Requested blocks per row.
    pub cols: u32,
    /// Requested blocks per column.
    pub rows: u32,
    /// Luma grid width.
    pub width: u32,
    /// Luma grid height.
    pub height: u32,
}

#[cfg(test)]
mod tests;
