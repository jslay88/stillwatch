//! `DpmsBlanker` against a private headless `KWin` (`kwin_wayland --virtual`
//! with two outputs, on its own D-Bus session). It never talks to the
//! desktop's compositor: both the watch and kscreen-doctor get the private
//! socket explicitly.
//!
//! Skipped when `kwin_wayland`, `kscreen-doctor`, or `dbus-run-session` is
//! missing, unless `STILLWATCH_REQUIRE_KWIN=1`. The launch code here is the
//! minimum this test needs; the shared headless `KWin` harness replaces it.

use std::os::unix::process::CommandExt as _;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rustix::process::{Pid, Signal, kill_process_group};
use stillwatch_core::backend::{Blanker, EventSink};
use stillwatch_core::event::Event;
use stillwatchd::action::dpms::DpmsBlanker;
use tokio::sync::mpsc;
use tokio::time::timeout;

const OUTPUTS: [&str; 2] = ["Virtual-0", "Virtual-1"];
const LIMIT: Duration = Duration::from_secs(15);

/// A headless `KWin` in its own process group, killed with everything it
/// started when dropped.
struct HeadlessKwin {
    child: Child,
    socket: String,
}

impl HeadlessKwin {
    fn start() -> Option<Self> {
        Self::start_numbered(0)
    }

    /// Starts `KWin` on a socket unique to this process and `n`, so tests can
    /// run side by side.
    fn start_numbered(n: u32) -> Option<Self> {
        let required = std::env::var_os("STILLWATCH_REQUIRE_KWIN").is_some_and(|v| v == "1");
        let missing: Vec<_> = ["kwin_wayland", "kscreen-doctor", "dbus-run-session"]
            .into_iter()
            .filter(|tool| !on_path(tool))
            .collect();
        let runtime = std::env::var_os("XDG_RUNTIME_DIR");
        if !missing.is_empty() || runtime.is_none() {
            assert!(
                !required,
                "STILLWATCH_REQUIRE_KWIN=1 but {missing:?} not found or XDG_RUNTIME_DIR unset"
            );
            eprintln!("skipping: {missing:?} not found or XDG_RUNTIME_DIR unset");
            return None;
        }

        let socket = format!("stillwatch-test-{}-{n}", std::process::id());
        let path = PathBuf::from(runtime?).join(&socket);
        let spawned = Command::new("dbus-run-session")
            .args(["--", "kwin_wayland", "--virtual", "--no-lockscreen"])
            .args(["--output-count", "2", "--width", "1280", "--height", "720"])
            .args(["--socket", &socket])
            .env_remove("DISPLAY")
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("WAYLAND_SOCKET")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn();
        assert!(spawned.is_ok(), "can't start kwin_wayland: {spawned:?}");
        let kwin = Self {
            child: spawned.ok()?,
            socket,
        };
        let started = Instant::now();
        while !path.exists() {
            assert!(
                started.elapsed() < LIMIT,
                "kwin_wayland never opened {}",
                path.display()
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        Some(kwin)
    }
}

impl Drop for HeadlessKwin {
    fn drop(&mut self) {
        let _ = kill_process_group(Pid::from_child(&self.child), Signal::TERM);
        let _ = self.child.wait();
    }
}

fn on_path(tool: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(tool).is_file()))
}

fn channel_sink() -> (Arc<dyn EventSink>, mpsc::UnboundedReceiver<Event>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let sink = move |event| {
        let _ = tx.send(event);
    };
    (Arc::new(sink), rx)
}

fn power(output: &str, on: bool) -> Event {
    Event::DisplayPower {
        output: output.to_owned(),
        on,
    }
}

/// Collects events until every output has reported `on`, sorted by output.
async fn states(rx: &mut mpsc::UnboundedReceiver<Event>, on: bool) -> Vec<Event> {
    let mut seen = Vec::new();
    let all_reported = |seen: &Vec<Event>| OUTPUTS.iter().all(|o| seen.contains(&power(o, on)));
    let _ = timeout(LIMIT, async {
        while !all_reported(&seen) {
            let Some(event) = rx.recv().await else {
                break;
            };
            seen.push(event);
        }
    })
    .await;
    assert!(
        all_reported(&seen),
        "expected every output on={on}, saw {seen:?}"
    );
    seen.sort_by_key(|event| format!("{event:?}"));
    seen
}

#[tokio::test]
async fn dpms_off_and_on_is_reported_per_output() {
    let Some(kwin) = HeadlessKwin::start() else {
        return;
    };
    let blanker = Arc::new(DpmsBlanker::new().on_display(&kwin.socket));
    let (sink, mut rx) = channel_sink();
    let watching = {
        let blanker = Arc::clone(&blanker);
        tokio::spawn(async move { blanker.watch(sink).await })
    };
    assert_eq!(states(&mut rx, true).await, OUTPUTS.map(|o| power(o, true)));

    // KWin 6.7.5 applies DPMS to the whole workspace, so the excluded output
    // goes dark too. If this starts failing because Virtual-1 stays on, KWin
    // honors --dpms-excluded now: update Platform facts and the dpms docs.
    blanker.blank(&[OUTPUTS[0].to_owned()]).await.unwrap();
    assert_eq!(
        states(&mut rx, false).await,
        OUTPUTS.map(|o| power(o, false))
    );

    blanker.unblank(&[]).await.unwrap();
    assert_eq!(states(&mut rx, true).await, OUTPUTS.map(|o| power(o, true)));
    assert!(!watching.is_finished());
    watching.abort();
}

#[tokio::test]
async fn blanking_an_unknown_output_changes_nothing() {
    let Some(kwin) = HeadlessKwin::start_numbered(1) else {
        return;
    };
    let blanker = DpmsBlanker::new().on_display(&kwin.socket);
    let error = blanker.blank(&["HDMI-A-9".to_owned()]).await.unwrap_err();
    assert!(error.to_string().contains("HDMI-A-9"), "{error}");
    blanker.unblank(&["HDMI-A-9".to_owned()]).await.unwrap();
}
