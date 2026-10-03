//! Test helpers for Stillwatch's D-Bus backends.
//!
//! [`PrivateBus`] runs a throwaway `dbus-daemon` per test so backends talk to
//! a real bus without touching the user's session. Fake services that sit on
//! that bus (such as [`mpris::FakePlayer`] and
//! [`notifications::FakeNotificationServer`]) live in submodules here so every
//! backend's integration tests share them instead of copying.
//!
//! When `dbus-daemon` isn't installed, [`PrivateBus::start`] returns
//! `Ok(None)` and the test should return early, unless
//! `STILLWATCH_REQUIRE_DBUS=1` is set (as in CI), which turns that into an
//! error.

mod bus;
mod error;
pub mod logind;
pub mod mpris;
pub mod notifications;
pub mod screensaver;
mod signal;
mod sync;

pub use bus::{PrivateBus, REQUIRE_ENV};
pub use error::Error;
