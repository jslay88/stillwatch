//! What an overlay looks like: solid black, or translucent black for dimming.

use smithay_client_toolkit::reexports::client::protocol::wl_shm::Format;

/// How an overlay covers its output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shade {
    /// Opaque black: the blank.
    Black,
    /// Black at this alpha (0 = clear, 255 = opaque): the dim.
    Dim {
        /// Overlay opacity.
        alpha: u8,
    },
}

impl Shade {
    /// The dim that leaves `percent` of the screen's brightness showing.
    #[must_use]
    pub fn dim(percent: u32) -> Self {
        Self::Dim {
            alpha: dim_alpha(percent),
        }
    }

    /// Whether this is a dim rather than the blank.
    #[must_use]
    pub const fn is_dim(self) -> bool {
        matches!(self, Self::Dim { .. })
    }

    /// The `wl_shm` format the pixel is in. Both are always supported.
    /// `Xrgb8888` tells the compositor the blank is opaque, so it can skip
    /// whatever is underneath.
    pub(super) const fn format(self) -> Format {
        match self {
            Self::Black => Format::Xrgb8888,
            Self::Dim { .. } => Format::Argb8888,
        }
    }

    /// One pixel in [`format`](Self::format), little-endian B, G, R, A.
    /// `wl_shm` alpha is premultiplied, and black stays all zeros.
    pub(super) const fn pixel(self) -> [u8; 4] {
        match self {
            Self::Black => [0, 0, 0, u8::MAX],
            Self::Dim { alpha } => [0, 0, 0, alpha],
        }
    }
}

/// Overlay alpha for `action.dim_percent`: `1 - dim_percent / 100`, scaled to
/// 0-255 and rounded. Black at that alpha leaves `dim_percent` of the
/// brightness showing, so 20 gives 204 and 100 gives a clear overlay.
/// Percentages above 100 count as 100.
#[must_use]
pub fn dim_alpha(dim_percent: u32) -> u8 {
    let covered = 100 - dim_percent.min(100);
    let alpha = (covered * u32::from(u8::MAX) + 50) / 100;
    u8::try_from(alpha).unwrap_or(u8::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alpha_follows_one_minus_dim_percent() {
        assert_eq!(dim_alpha(20), 204);
        assert_eq!(dim_alpha(0), 255);
        assert_eq!(dim_alpha(50), 128);
        assert_eq!(dim_alpha(100), 0);
        assert_eq!(dim_alpha(1), 252);
        assert_eq!(dim_alpha(99), 3);
    }

    #[test]
    fn out_of_range_percentages_clamp_to_no_dim() {
        assert_eq!(dim_alpha(101), 0);
        assert_eq!(dim_alpha(u32::MAX), 0);
    }

    #[test]
    fn every_percentage_rounds_the_covered_fraction() {
        let mut previous = u8::MAX;
        for percent in 0..=100 {
            let alpha = dim_alpha(percent);
            let exact = f64::from(100 - percent) * 255.0 / 100.0;
            assert!(
                (f64::from(alpha) - exact).abs() <= 0.5 + 1e-9,
                "{percent}% -> {alpha}, want ~{exact}"
            );
            assert!(alpha <= previous, "{percent}% got darker");
            previous = alpha;
        }
    }

    #[test]
    fn blank_is_opaque_black_and_dim_is_translucent() {
        assert_eq!(Shade::Black.pixel(), [0, 0, 0, 255]);
        assert_eq!(Shade::Black.format(), Format::Xrgb8888);
        assert!(!Shade::Black.is_dim());

        let dim = Shade::dim(20);
        assert_eq!(dim, Shade::Dim { alpha: 204 });
        assert_eq!(dim.pixel(), [0, 0, 0, 204]);
        assert_eq!(dim.format(), Format::Argb8888);
        assert!(dim.is_dim());
    }
}
