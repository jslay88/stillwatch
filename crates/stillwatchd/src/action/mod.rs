//! What Stillwatch does once the prompt times out: blank methods and hooks.
//!
//! [`ActionRunner`] executes the machine's `Blank`, `Unblank`, `Lock`, and
//! `RunHook` commands against the blankers, the session monitor, and
//! `sh -c`. It does not own any `watch`: the daemon loop supervises those
//! on the same backend instances it hands in.

pub mod ddc;
pub mod dpms;

mod brightness;
mod hooks;
mod runner;
mod targets;

pub use brightness::BrightnessDimmer;
pub use hooks::{ENV_METHOD, ENV_OUTPUTS, ENV_REASON, HOOK_TIMEOUT, hook_spec};
pub use runner::{ActionBackends, ActionRunner};
pub use targets::resolve_outputs;

#[cfg(test)]
mod tests;
