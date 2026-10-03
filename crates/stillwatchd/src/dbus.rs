//! Which D-Bus bus a backend talks to.

use zbus::connection::Builder;

/// The bus a backend connects to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Bus {
    /// The user's session bus.
    Session,
    /// A bus at an explicit address (tests use a private bus).
    Address(String),
}

impl Bus {
    /// A connection builder for this bus.
    pub(crate) fn builder(&self) -> zbus::Result<Builder<'static>> {
        match self {
            Self::Session => Builder::session(),
            Self::Address(address) => Builder::address(address.as_str()),
        }
    }
}
