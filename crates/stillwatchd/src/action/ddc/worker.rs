//! Runs blocking DDC/CI calls off the async runtime, with a time limit and
//! bounded retries.

use std::time::Duration;

use tokio::{task, time};

use super::{DdcError, Timing};

/// Runs `op` on the blocking pool and gives up after `limit`. A call that
/// times out keeps running on its thread; only the wait is abandoned.
pub(crate) async fn run<T, F>(what: &str, limit: Duration, op: F) -> Result<T, DdcError>
where
    F: FnOnce() -> Result<T, DdcError> + Send + 'static,
    T: Send + 'static,
{
    match time::timeout(limit, task::spawn_blocking(op)).await {
        Ok(Ok(result)) => result,
        Ok(Err(join)) => Err(DdcError::Unavailable(format!(
            "the DDC/CI {what} worker died: {join}"
        ))),
        Err(_) => Err(DdcError::Timeout {
            op: what.to_owned(),
            after: limit,
        }),
    }
}

/// [`run`] with `timing.io_timeout`, repeating retryable failures up to
/// `timing.attempts` tries in all. Displays often refuse a DDC/CI command or
/// answer with a bad checksum once, then succeed.
pub(crate) async fn retry<T, F>(timing: &Timing, what: &str, op: F) -> Result<T, DdcError>
where
    F: Fn() -> Result<T, DdcError> + Clone + Send + 'static,
    T: Send + 'static,
{
    let mut attempt = 1;
    loop {
        match run(what, timing.io_timeout, op.clone()).await {
            Err(error) if error.is_retryable() && attempt < timing.attempts => {
                tracing::debug!(%error, attempt, "DDC/CI {what} failed, retrying");
                time::sleep(timing.retry_delay).await;
                attempt += 1;
            }
            result => return result,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    fn failing(times: u32) -> (Arc<AtomicU32>, impl Fn() -> Result<u32, DdcError> + Clone) {
        let calls = Arc::new(AtomicU32::new(0));
        let counter = Arc::clone(&calls);
        let op = move || {
            let call = counter.fetch_add(1, Ordering::SeqCst) + 1;
            if call <= times {
                Err(DdcError::ReadFailed {
                    output: "HDMI-A-1".into(),
                    code: 0xD6,
                    detail: format!("NAK {call}"),
                })
            } else {
                Ok(call)
            }
        };
        (calls, op)
    }

    #[tokio::test(start_paused = true)]
    async fn retries_until_success() {
        let (calls, op) = failing(2);
        let start = time::Instant::now();
        assert_eq!(retry(&Timing::default(), "read", op).await, Ok(3));
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert_eq!(start.elapsed(), Timing::default().retry_delay * 2);
    }

    #[tokio::test(start_paused = true)]
    async fn gives_up_after_the_last_attempt() {
        let (calls, op) = failing(10);
        let error = retry(&Timing::default(), "read", op).await.unwrap_err();
        assert!(error.to_string().ends_with("NAK 3"), "{error}");
        assert_eq!(calls.load(Ordering::SeqCst), Timing::default().attempts);
    }

    #[tokio::test]
    async fn permanent_errors_are_not_retried() {
        let calls = Arc::new(AtomicU32::new(0));
        let counter = Arc::clone(&calls);
        let op = move || -> Result<(), DdcError> {
            counter.fetch_add(1, Ordering::SeqCst);
            Err(DdcError::Unavailable("gone".into()))
        };
        assert_eq!(
            retry(&Timing::default(), "write", op).await,
            Err(DdcError::Unavailable("gone".into()))
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn slow_calls_time_out() {
        let limit = Duration::from_millis(10);
        let result = run("write", limit, || {
            std::thread::sleep(Duration::from_millis(200));
            Ok(())
        })
        .await;
        assert_eq!(
            result,
            Err(DdcError::Timeout {
                op: "write".into(),
                after: limit
            })
        );
    }

    #[tokio::test]
    async fn a_panicking_call_is_an_error() {
        let result: Result<(), DdcError> = run("scan", Duration::from_secs(5), || {
            panic!("driver bug");
        })
        .await;
        assert!(
            matches!(&result, Err(DdcError::Unavailable(m)) if m.starts_with("the DDC/CI scan worker died")),
            "{result:?}"
        );
    }
}
