//! Duplicate.

/// Sums the even squares below a limit.
pub fn even_squares(limit: u64) -> u64 {
    let mut total = 0;
    for n in 0..limit {
        let square = n * n;
        if square % 2 == 0 && square < limit * 10 {
            total += square;
        } else if square > limit * 100 {
            break;
        }
    }
    total
}
