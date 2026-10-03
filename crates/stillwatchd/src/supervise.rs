//! Keeps a backend's long-running `watch` alive across dropped connections.

use std::future::Future;

use stillwatch_core::backend::BackendError;
use stillwatch_core::backoff::Backoff;
use stillwatch_core::time::Clock;

/// Runs `attempt` until it ends cleanly or fails for good.
///
/// Transient errors ([`BackendError::is_transient`], such as a dropped
/// connection) are logged and retried after `backoff`'s next delay. An
/// attempt that ran for the policy's `reset_after` counts as healthy, so the
/// delay after it starts over. `clock` measures how long each attempt ran.
///
/// Dropping the returned future cancels the current attempt or sleep.
///
/// # Errors
///
/// Returns the first non-transient error, for example
/// [`BackendError::Unsupported`].
pub async fn supervise<F, Fut>(
    name: &str,
    mut backoff: Backoff,
    clock: &dyn Clock,
    mut attempt: F,
) -> Result<(), BackendError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<(), BackendError>>,
{
    loop {
        let started = clock.now();
        match attempt().await {
            Ok(()) => {
                tracing::info!(backend = name, "backend ended");
                return Ok(());
            }
            Err(error) if error.is_transient() => {
                let ran_for = clock.now().saturating_duration_since(started);
                let delay = backoff.after_attempt(ran_for);
                tracing::warn!(
                    backend = name,
                    %error,
                    failures = backoff.failures(),
                    ?ran_for,
                    ?delay,
                    "backend lost its connection, reconnecting after a delay"
                );
                tokio::time::sleep(delay).await;
                tracing::info!(backend = name, attempt = backoff.failures(), "reconnecting");
            }
            Err(error) => {
                tracing::error!(backend = name, %error, "backend failed, not retrying");
                return Err(error);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use stillwatch_core::backoff::BackoffPolicy;
    use stillwatch_core::time::FakeClock;
    use tokio::time::Instant;

    use super::*;

    /// One scripted attempt: how long it "runs" on the fake clock, then its
    /// result.
    type Step = (u64, Result<(), BackendError>);

    fn lost() -> Result<(), BackendError> {
        Err(BackendError::Disconnected("socket closed".into()))
    }

    fn policy() -> Backoff {
        Backoff::new(BackoffPolicy {
            initial: Duration::from_secs(1),
            max: Duration::from_secs(8),
            multiplier: 2,
            reset_after: Duration::from_secs(60),
        })
    }

    /// Runs `supervise` over `steps` and returns its result plus the paused
    /// tokio time (in seconds since the start) at which each attempt began.
    async fn run(steps: Vec<Step>) -> (Result<(), BackendError>, Vec<u64>) {
        let clock = FakeClock::new();
        let start = Instant::now();
        let steps = Arc::new(Mutex::new(VecDeque::from(steps)));
        let starts = Arc::new(Mutex::new(Vec::new()));
        let result = supervise("test", policy(), &clock, || {
            starts.lock().unwrap().push(start.elapsed().as_secs());
            let (ran_for, result) = steps.lock().unwrap().pop_front().unwrap();
            clock.advance(Duration::from_secs(ran_for));
            std::future::ready(result)
        })
        .await;
        let starts = starts.lock().unwrap().clone();
        (result, starts)
    }

    #[tokio::test(start_paused = true)]
    async fn clean_end_returns_ok_without_retrying() {
        assert_eq!(run(vec![(0, Ok(()))]).await, (Ok(()), vec![0]));
    }

    #[tokio::test(start_paused = true)]
    async fn transient_failures_back_off_exponentially_up_to_the_cap() {
        let steps = vec![
            (0, lost()),
            (0, lost()),
            (0, Err(BackendError::Io("EPIPE".into()))),
            (0, lost()),
            (0, lost()),
            (0, Ok(())),
        ];
        let (result, starts) = run(steps).await;
        assert_eq!(result, Ok(()));
        assert_eq!(starts, [0, 1, 3, 7, 15, 23]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_healthy_run_resets_the_delay() {
        let steps = vec![(0, lost()), (0, lost()), (120, lost()), (0, Ok(()))];
        let (result, starts) = run(steps).await;
        assert_eq!(result, Ok(()));
        assert_eq!(starts, [0, 1, 3, 4]);
    }

    #[tokio::test(start_paused = true)]
    async fn permanent_errors_stop_immediately() {
        let unsupported = BackendError::Unsupported("ext-idle-notify v1".into());
        let steps = vec![(0, lost()), (0, Err(unsupported.clone())), (0, Ok(()))];
        let (result, starts) = run(steps).await;
        assert_eq!(result, Err(unsupported));
        assert_eq!(starts, [0, 1]);
    }
}
