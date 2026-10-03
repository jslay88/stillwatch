//! The settings window on a headless compositor, not the desktop session.
//!
//! winit uses Wayland when `WAYLAND_DISPLAY` is set and X11 when only
//! `DISPLAY` is. This sandbox sets the first and not the second.

use std::io::{BufRead, BufReader};
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use stillwatch_testkit::kwin::{Kwin, KwinOptions};

const READY: Duration = Duration::from_secs(30);

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
async fn settings_speaks_wayland_when_the_display_is_set() {
    let Some(kwin) = Kwin::start(KwinOptions::default()).await.unwrap() else {
        eprintln!("skipping: kwin_wayland is not installed");
        return;
    };
    let mut child = kwin
        .command(env!("CARGO_BIN_EXE_stillwatch-gui"))
        .arg("settings")
        .env("WAYLAND_DEBUG", "1")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_SOCKET")
        .stderr(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let stderr = child.stderr.take().unwrap();
    let mut guard = ChildGuard(Some(child));
    let log = Arc::new(Mutex::new(String::new()));
    let recorded = Arc::clone(&log);
    let (tx, rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            let Ok(line) = line else {
                break;
            };
            let hit = line.contains("wl_compositor") || line.contains("xdg_wm_base");
            {
                let mut log = recorded.lock().unwrap();
                if log.len() < 4096 {
                    log.push_str(&line);
                    log.push('\n');
                }
            }
            if hit {
                let _ = tx.send(());
            }
        }
    });
    let connected = rx.recv_timeout(READY).is_ok();
    let child = guard.0.as_mut().unwrap();
    let still_running = child.try_wait().unwrap().is_none();
    let log = log.lock().unwrap().clone();
    assert!(
        connected,
        "settings did not speak Wayland on the headless compositor:\n{log}"
    );
    assert!(
        still_running,
        "settings exited before the window was up:\n{log}"
    );
}
