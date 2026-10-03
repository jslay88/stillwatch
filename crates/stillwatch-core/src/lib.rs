//! Pure logic for Stillwatch.
//!
//! This crate holds the config schema and validation, the stale-image detector,
//! the state machine, and the backend contracts. It performs no I/O apart from
//! parsing, so everything here is unit-testable with fakes.

/// The crate version, as declared in `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    #[test]
    fn version_is_set() {
        assert_eq!(super::VERSION, env!("CARGO_PKG_VERSION"));
    }
}
