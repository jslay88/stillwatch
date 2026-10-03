//! The session monitor against a fake logind and screensaver, on two private
//! buses standing in for the system and session buses.

mod events;
mod failures;
mod harness;
mod lock;
mod state;
