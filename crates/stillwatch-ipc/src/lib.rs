//! D-Bus interface definitions and wire types for Stillwatch.
//!
//! The daemon serves the `io.github.jslay88.Stillwatch1` interface at
//! [`OBJECT_PATH`] under the well-known name [`BUS_NAME`]; the CLI and GUI use
//! the proxy in [`proxy`]. Structured payloads travel as JSON strings of the
//! types in [`status`], [`probe`], and [`gamepad`], built with [`json`].
//! Process-level helpers shared by all three binaries (logging setup, standard
//! paths, and reading the config file) live here too.

pub mod config_file;
pub mod error;
pub mod gamepad;
pub mod json;
pub mod logging;
pub mod paths;
pub mod probe;
pub mod prompt;
pub mod proxy;
pub mod status;

/// Well-known bus name of the Stillwatch daemon.
pub const BUS_NAME: &str = "io.github.jslay88.Stillwatch";

/// Object path the daemon serves its interface at.
pub const OBJECT_PATH: &str = "/io/github/jslay88/Stillwatch";

/// Versioned D-Bus interface name.
pub const INTERFACE: &str = "io.github.jslay88.Stillwatch1";

#[cfg(test)]
mod tests {
    #[test]
    fn bus_name_is_reverse_dns() {
        assert_eq!(super::BUS_NAME.split('.').count(), 4);
    }

    #[test]
    fn path_and_interface_derive_from_the_bus_name() {
        assert_eq!(
            super::OBJECT_PATH,
            format!("/{}", super::BUS_NAME.replace('.', "/"))
        );
        assert_eq!(super::INTERFACE, format!("{}1", super::BUS_NAME));
    }
}
