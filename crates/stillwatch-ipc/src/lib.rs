//! D-Bus interface definitions and wire types for Stillwatch.
//!
//! The daemon serves the `io.github.jslay88.Stillwatch` interface; the CLI and
//! GUI use the proxies defined here.

/// Well-known bus name of the Stillwatch daemon.
pub const BUS_NAME: &str = "io.github.jslay88.Stillwatch";

#[cfg(test)]
mod tests {
    #[test]
    fn bus_name_is_reverse_dns() {
        assert_eq!(super::BUS_NAME.split('.').count(), 4);
    }
}
