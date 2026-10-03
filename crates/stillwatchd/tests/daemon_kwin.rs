//! The daemon against a headless `kwin_wayland --virtual`.
//!
//! It has to come up, own the bus name, and report Active. The idle timeout
//! stays at the default, so this never blanks the virtual outputs. The
//! `Kwin` sandbox is killed on drop, and so is the daemon child.

use std::process::{Child, Command, Stdio};
use std::time::Duration;

use stillwatch_ipc::BUS_NAME;
use stillwatch_ipc::proxy::StillwatchProxy;
use stillwatch_testkit::kwin::{Kwin, KwinOptions};
use tokio::time::Instant;

const WAIT: Duration = Duration::from_secs(30);

struct ChildGuard(Option<Child>);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[tokio::test]
async fn starts_owns_the_bus_name_and_reports_active() {
    let Some(kwin) = Kwin::start(KwinOptions::default()).await.unwrap() else {
        return;
    };
    let child = kwin
        .command(env!("CARGO_BIN_EXE_stillwatchd"))
        .args([
            "--config",
            "/nonexistent/stillwatch/config.toml",
            "--log-level",
            "info",
        ])
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut guard = ChildGuard(Some(child));
    let child = guard.0.as_mut().unwrap();

    kwin.wait_for_name(BUS_NAME, WAIT).await.unwrap();
    let conn = kwin.bus().connect().await.unwrap();
    let proxy = StillwatchProxy::new(&conn).await.unwrap();
    let status = proxy.status().await.unwrap();
    assert!(status.contains(r#""state":"active""#), "{status}");

    let signaled = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(signaled.success());

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "{status}");
            break;
        }
        assert!(Instant::now() < deadline, "stillwatchd didn't exit");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
