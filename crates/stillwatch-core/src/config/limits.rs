//! Allowed ranges for numeric settings, shared by validation and the settings schema.

use std::fmt;

/// An inclusive range of allowed values for a numeric setting.
///
/// A `max` of [`u32::MAX`] means there is no upper limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    /// Smallest allowed value.
    pub min: u32,
    /// Largest allowed value.
    pub max: u32,
}

impl Bounds {
    /// Values from `min` to `max`, inclusive.
    #[must_use]
    pub const fn new(min: u32, max: u32) -> Self {
        Self { min, max }
    }

    /// Values of `min` or more, with no upper limit.
    #[must_use]
    pub const fn at_least(min: u32) -> Self {
        Self::new(min, u32::MAX)
    }

    /// Whether `value` is allowed.
    #[must_use]
    pub const fn contains(self, value: u32) -> bool {
        self.min <= value && value <= self.max
    }

    /// The upper limit, or `None` when there isn't one.
    #[must_use]
    pub const fn upper(self) -> Option<u32> {
        if self.max == u32::MAX {
            None
        } else {
            Some(self.max)
        }
    }
}

impl fmt::Display for Bounds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.upper() {
            Some(max) => write!(f, "{} to {max}", self.min),
            None => write!(f, "at least {}", self.min),
        }
    }
}

/// Any non-negative value; for settings with no rule of their own.
pub const ANY: Bounds = Bounds::at_least(0);

/// At least 1.
pub const POSITIVE: Bounds = Bounds::at_least(1);

/// A percentage where 0 is allowed (often meaning "disabled").
pub const PERCENT: Bounds = Bounds::new(0, 100);

/// A percentage that must be at least 1.
pub const PERCENT_NONZERO: Bounds = Bounds::new(1, 100);

/// An 8-bit luma value.
pub const LUMA: Bounds = Bounds::new(0, 255);

/// Width of the downscaled luma grid. Narrower grids lose too much detail.
pub const DOWNSCALE_WIDTH: Bounds = Bounds::at_least(16);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains_is_inclusive() {
        assert!(PERCENT.contains(0));
        assert!(PERCENT.contains(100));
        assert!(!PERCENT.contains(101));
        assert!(!POSITIVE.contains(0));
        assert!(POSITIVE.contains(u32::MAX));
    }

    #[test]
    fn open_bounds_have_no_upper_limit() {
        assert_eq!(POSITIVE.upper(), None);
        assert_eq!(LUMA.upper(), Some(255));
    }

    #[test]
    fn display_reads_naturally() {
        assert_eq!(PERCENT_NONZERO.to_string(), "1 to 100");
        assert_eq!(DOWNSCALE_WIDTH.to_string(), "at least 16");
        assert_eq!(ANY.to_string(), "at least 0");
    }
}
