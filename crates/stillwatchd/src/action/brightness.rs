//! KDE `org.kde.ScreenBrightness` dimming. Not implemented: talking to
//! `PowerDevil` from the daemon would hit the real session bus. The daemon
//! wiring can swap in a real dimmer once there is a fake-bus test for it.

use stillwatch_core::backend::{BackendError, BackendFuture, Dimmer};

/// A [`Dimmer`] that always returns [`BackendError::Unavailable`].
///
/// [`ActionRunner`](super::ActionRunner) falls back to the overlay dimmer
/// when this fails, and logs the error.
#[derive(Debug, Clone, Copy, Default)]
pub struct BrightnessDimmer;

impl Dimmer for BrightnessDimmer {
    fn dim<'a>(&'a self, _outputs: &'a [String], _percent: u32) -> BackendFuture<'a, ()> {
        Box::pin(std::future::ready(Err(unavailable())))
    }

    fn undim<'a>(&'a self, _outputs: &'a [String]) -> BackendFuture<'a, ()> {
        Box::pin(std::future::ready(Err(unavailable())))
    }
}

fn unavailable() -> BackendError {
    BackendError::Unavailable(
        "org.kde.ScreenBrightness dimming is not implemented; using the overlay".into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use stillwatch_core::mocks::now_or_never;

    #[test]
    fn dim_and_undim_are_unavailable() {
        let dimmer = BrightnessDimmer;
        assert!(matches!(
            now_or_never(dimmer.dim(&[], 20)),
            Some(Err(BackendError::Unavailable(_)))
        ));
        assert!(matches!(
            now_or_never(dimmer.undim(&[])),
            Some(Err(BackendError::Unavailable(_)))
        ));
    }
}
