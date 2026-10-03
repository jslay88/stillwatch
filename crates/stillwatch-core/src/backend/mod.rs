//! Backend contracts.
//!
//! Each trait is object-safe and `Send + Sync`, so the daemon holds backends as
//! `Arc<dyn Trait>` and tests swap in the mocks from `crate::mocks`. Async
//! methods return a [`BoxFuture`]; dropping that future cancels the operation
//! and releases whatever it held.
//!
//! Long-running `watch` methods push [`Event`](crate::event::Event)s into an
//! [`EventSink`] until they fail. They return `Err` when the underlying
//! connection is lost so the caller can back off and call `watch` again, and
//! `Ok(())` only when the source ended cleanly (for example a mock whose
//! script ran out).

mod blanker;
mod capture;
mod dimmer;
mod error;
mod gamepad;
mod history;
mod idle;
mod media;
mod prompter;
mod session;
mod sink;

use std::future::Future;
use std::pin::Pin;

pub use blanker::Blanker;
pub use capture::ScreenCapture;
pub use dimmer::Dimmer;
pub use error::BackendError;
pub use gamepad::{GamepadDevice, GamepadSource};
pub use history::HistorySink;
pub use idle::IdleSource;
pub use media::MediaWatcher;
pub use prompter::Prompter;
pub use session::SessionMonitor;
pub use sink::EventSink;

/// A boxed, `Send` future, so async trait methods stay object-safe.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Shorthand for a backend operation's future.
pub type BackendFuture<'a, T> = BoxFuture<'a, Result<T, BackendError>>;

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::mocks::{
        MemoryHistory, MockBlanker, MockCapture, MockGamepadSource, MockIdleSource,
        MockMediaWatcher, MockPrompter, MockSessionMonitor, RecordingSink,
    };

    #[test]
    fn every_trait_is_object_safe_and_mocked() {
        let _: Arc<dyn IdleSource> = Arc::new(MockIdleSource::new());
        let _: Arc<dyn GamepadSource> = Arc::new(MockGamepadSource::new());
        let _: Arc<dyn ScreenCapture> = Arc::new(MockCapture::new());
        let _: Arc<dyn Prompter> = Arc::new(MockPrompter::new());
        let _: Arc<dyn Blanker> = Arc::new(MockBlanker::new());
        let _: Arc<dyn Dimmer> = Arc::new(MockBlanker::new());
        let _: Arc<dyn SessionMonitor> = Arc::new(MockSessionMonitor::new());
        let _: Arc<dyn MediaWatcher> = Arc::new(MockMediaWatcher::new());
        let _: Arc<dyn HistorySink> = Arc::new(MemoryHistory::new());
        let _: Arc<dyn EventSink> = Arc::new(RecordingSink::new());
    }
}
