//! `stillwatch probe` against a fake daemon on a private session bus: the
//! samples it prints, and that the probe always stops afterwards.

mod support;

use std::time::Duration;

use jiff::Timestamp;
use jiff::tz::TimeZone;
use stillwatch_cli::exit;
use stillwatch_cli::render::{self, Style};
use stillwatch_core::stats::{BlockCounts, BlockState, OutputStats, Threshold, ThresholdReason};
use stillwatch_ipc::json::from_json;
use stillwatch_ipc::probe::{ProbeOutput, ProbeSample};
use support::{Daemon, TestResult, WAIT, stillwatch, stillwatch_until};
use tokio::time::{sleep, timeout};

fn sample() -> ProbeSample {
    let blocks = vec![
        BlockState::Persistent,
        BlockState::Persistent,
        BlockState::Changed,
        BlockState::Dark,
    ];
    let stats = OutputStats::from_counts("HDMI-A-1", BlockCounts::from_states(&blocks), 70);
    ProbeSample {
        at: Timestamp::constant(1_790_000_000, 0),
        threshold: Threshold::new(70, ThresholdReason::Normal),
        stale: false,
        outputs: vec![ProbeOutput {
            stats,
            columns: 2,
            rows: 2,
            blocks,
        }],
    }
}

async fn daemon_with_sample() -> TestResult<Option<Daemon>> {
    let daemon = Daemon::start().await?;
    if let Some(daemon) = &daemon {
        daemon.fake.update(|state| state.sample = sample());
    }
    Ok(daemon)
}

#[tokio::test]
async fn json_prints_count_samples_then_stops_the_probe() {
    let Some(daemon) = daemon_with_sample().await.unwrap() else {
        return;
    };
    let args = ["probe", "--interval", "100ms", "--count", "3", "--json"];
    let ran = stillwatch(daemon.address(), &args).await.unwrap();
    ran.result.unwrap();
    let lines: Vec<&str> = ran.out.lines().collect();
    assert_eq!(lines.len(), 3, "{}", ran.out);
    for line in lines {
        assert_eq!(from_json::<ProbeSample>(line).unwrap(), sample());
        assert!(!line.contains("luma"));
    }
    timeout(WAIT, daemon.fake.wait_for_probes(0)).await.unwrap();
    assert_eq!(
        daemon.fake.state().probe_intervals,
        [Duration::from_millis(100)]
    );
}

#[tokio::test]
async fn text_uses_the_shared_grid_renderer() {
    let Some(daemon) = daemon_with_sample().await.unwrap() else {
        return;
    };
    let ran = stillwatch(
        daemon.address(),
        &["probe", "--interval", "1s", "--count", "1"],
    )
    .await
    .unwrap();
    ran.result.unwrap();
    assert_eq!(
        ran.out,
        render::probe::render(&sample(), &Style::plain(TimeZone::UTC))
    );
    assert!(ran.out.contains("\n████\n░░··\n"), "{}", ran.out);
    timeout(WAIT, daemon.fake.wait_for_probes(0)).await.unwrap();
}

#[tokio::test]
async fn stopping_early_stops_the_probe() {
    let Some(daemon) = daemon_with_sample().await.unwrap() else {
        return;
    };
    let fake = &daemon.fake;
    let stop = async {
        fake.wait_for_probes(1).await;
        sleep(Duration::from_millis(250)).await;
    };
    let args = ["probe", "--interval", "100ms", "--json"];
    let ran = stillwatch_until(daemon.address(), &args, stop)
        .await
        .unwrap();
    ran.result.unwrap();
    assert!(ran.out.lines().count() >= 1, "{}", ran.out);
    timeout(WAIT, daemon.fake.wait_for_probes(0)).await.unwrap();
}

#[tokio::test]
async fn a_too_short_interval_is_refused() {
    let Some(daemon) = daemon_with_sample().await.unwrap() else {
        return;
    };
    let ran = stillwatch(daemon.address(), &["probe", "--interval", "50ms"])
        .await
        .unwrap();
    assert_eq!(ran.error(), "probe interval must be at least 100 ms");
    assert_eq!(ran.exit_code(), exit::FAILED);
    assert_eq!(daemon.fake.live_probes(), 0);
}

#[tokio::test]
async fn a_huge_interval_is_refused_before_asking() {
    let Some(daemon) = daemon_with_sample().await.unwrap() else {
        return;
    };
    let ran = stillwatch(daemon.address(), &["probe", "--interval", "60days"])
        .await
        .unwrap();
    assert!(
        ran.error().starts_with("--interval can be at most"),
        "{}",
        ran.error()
    );
    assert_eq!(daemon.fake.state().probe_intervals.len(), 0);
}

#[tokio::test]
async fn the_daemon_stopping_ends_the_probe() {
    let Some(daemon) = daemon_with_sample().await.unwrap() else {
        return;
    };
    let args = ["probe", "--interval", "100ms", "--json"];
    let probe = stillwatch(daemon.address(), &args);
    let kill = async {
        daemon.fake.wait_for_probes(1).await;
        daemon.service.connection().clone().close().await.unwrap();
    };
    let (ran, ()) = timeout(WAIT, async { tokio::join!(probe, kill) })
        .await
        .unwrap();
    let ran = ran.unwrap();
    assert_eq!(
        ran.error(),
        "stillwatchd stopped; start it again with systemctl --user start stillwatch"
    );
    assert_eq!(ran.exit_code(), exit::NOT_RUNNING);
}
