//! Scriptable mock backends for tests.
//!
//! Compiled for this crate's tests and, through the `mocks` feature, for other
//! crates' tests:
//!
//! ```toml
//! [dev-dependencies]
//! stillwatch-core = { workspace = true, features = ["mocks"] }
//! ```
//!
//! Every mock is `Send + Sync` and uses interior mutability, so a test keeps
//! an `Arc` to script and inspect it while the code under test holds it as an
//! `Arc<dyn Trait>`. Mock futures don't need a runtime: they are ready on the
//! first poll unless a script says to hang, so [`now_or_never`] can drive
//! them in plain `#[test]`s.

mod blanker;
mod capture;
mod detector;
mod gamepad;
mod harness;
mod history;
mod idle;
mod media;
mod prompter;
mod script;
mod session;
mod sink;

pub use blanker::{BlankerCall, MockBlanker};
pub use capture::MockCapture;
pub use detector::{Observation, ScriptedDetector};
pub use gamepad::MockGamepadSource;
pub use harness::Harness;
pub use history::MemoryHistory;
pub use idle::MockIdleSource;
pub use media::MockMediaWatcher;
pub use prompter::MockPrompter;
pub use script::{CallLog, Script, ScriptedWatch, WatchEnd, WatchRun, now_or_never};
pub use session::MockSessionMonitor;
pub use sink::RecordingSink;
