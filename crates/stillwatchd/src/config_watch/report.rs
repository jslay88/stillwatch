//! The one reload path: every trigger reloads here and reports once.

use std::future::Future;

use super::{ReloadOutcome, ReloadTrigger, Reloader};
use crate::service::{ReloadReport, ServiceError, ServiceSignals};

/// Where `ConfigChanged` goes. [`ServiceSignals`] in the daemon.
pub trait ReloadSignal {
    /// Emits `ConfigChanged(ok, errors)` for one reload attempt.
    fn config_changed(
        &self,
        report: &ReloadReport,
    ) -> impl Future<Output = Result<(), ServiceError>> + Send;
}

impl ReloadSignal for ServiceSignals {
    fn config_changed(
        &self,
        report: &ReloadReport,
    ) -> impl Future<Output = Result<(), ServiceError>> + Send {
        Self::config_changed(self, report)
    }
}

/// Reloads for `trigger` and emits `ConfigChanged` exactly once if it was an
/// attempt (anything but [`ReloadOutcome::Unchanged`]).
///
/// A signal that can't be sent is logged; the reload still counts. The
/// caller applies the outcome (see [`ReloadOutcome::update_machine`]) and,
/// for D-Bus `Reload()`, returns [`ReloadOutcome::report`] to the caller.
pub async fn reload_and_report(
    reloader: &mut Reloader,
    trigger: ReloadTrigger,
    signal: &impl ReloadSignal,
) -> ReloadOutcome {
    let outcome = reloader.reload(trigger);
    if let Some(report) = outcome.report()
        && let Err(error) = signal.config_changed(&report).await
    {
        tracing::warn!(%error, "couldn't emit ConfigChanged");
    }
    outcome
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[derive(Default)]
    struct Unreachable(AtomicUsize);

    impl ReloadSignal for Unreachable {
        fn config_changed(
            &self,
            _: &ReloadReport,
        ) -> impl Future<Output = Result<(), ServiceError>> + Send {
            self.0.fetch_add(1, Ordering::Relaxed);
            std::future::ready(Err(zbus::Error::Failure("bus is gone".into()).into()))
        }
    }

    #[tokio::test]
    async fn a_failed_signal_doesnt_undo_the_reload() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut reloader, _) = Reloader::load(tmp.path().join("config.toml")).unwrap();
        let signal = Unreachable::default();

        let outcome = reload_and_report(&mut reloader, ReloadTrigger::Requested, &signal).await;
        assert!(matches!(outcome, ReloadOutcome::Applied(_)));
        let outcome = reload_and_report(&mut reloader, ReloadTrigger::FileChanged, &signal).await;
        assert!(matches!(outcome, ReloadOutcome::Unchanged));
        assert_eq!(signal.0.load(Ordering::Relaxed), 1);
    }
}
