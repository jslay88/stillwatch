//! `KwinCapture` against a real headless `KWin` (`kwin_wayland --virtual`),
//! including the `.desktop` authorization. Skipped unless the harness sets
//! `STILLWATCH_KWIN_HARNESS=1`.
//!
//! What the harness has to provide:
//!
//! - `DBUS_SESSION_BUS_ADDRESS` and `WAYLAND_DISPLAY` for a private session
//!   bus and a `kwin_wayland --virtual` running on it, with `OpenGL`
//!   compositing (Mesa llvmpipe is fine). The `QPainter` backend can't take
//!   screenshots and answers `Cancelled`.
//! - `STILLWATCH_KWIN_APPLICATIONS_DIR`: an `applications/` directory `KWin`
//!   searches for `.desktop` files and this test can write to, for example
//!   `$XDG_DATA_HOME/applications` where `XDG_DATA_HOME` is the one `KWin`
//!   was started with. Give `KWin` its own `XDG_CACHE_HOME` too, so its
//!   `ksycoca6` cache isn't shared with the desktop.
//! - Optionally `STILLWATCH_KWIN_OUTPUT_SIZE=WIDTHxHEIGHT`, the size `KWin`
//!   was started with (`--width`/`--height`), checked against the capture.
//!
//! Authorization is per executable, and every test in this binary is the
//! same executable, so both halves run in one test, in order.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use stillwatch_core::backend::{BackendError, ScreenCapture as _};
use stillwatchd::capture::kwin::{Capture, KwinCapture};

const AUTHORIZE_TIMEOUT: Duration = Duration::from_secs(15);

/// Removes the test's `.desktop` file however the test ends.
struct DesktopFile(PathBuf);

impl DesktopFile {
    fn install(dir: &Path) -> io::Result<Self> {
        let exe = std::env::current_exe()?;
        let path = dir.join(format!("stillwatch-test-{}.desktop", std::process::id()));
        let contents = format!(
            "[Desktop Entry]\nType=Application\nName=Stillwatch test\nExec=\"{}\"\n\
             NoDisplay=true\nX-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2\n",
            exe.display()
        );
        std::fs::write(&path, contents)?;
        Ok(Self(path))
    }
}

impl Drop for DesktopFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn expected_size() -> Option<(u32, u32)> {
    let size = std::env::var("STILLWATCH_KWIN_OUTPUT_SIZE").ok()?;
    let (width, height) = size.split_once('x')?;
    Some((width.parse().ok()?, height.parse().ok()?))
}

/// `KWin` rebuilds its service cache when the applications directory
/// changes, which can lag the write slightly.
async fn capture_once_authorized(
    capture: &KwinCapture,
    output: &str,
) -> Result<Capture, BackendError> {
    let started = Instant::now();
    loop {
        match capture.capture(output, 480).await {
            Err(BackendError::PermissionDenied(_)) if started.elapsed() < AUTHORIZE_TIMEOUT => {
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            result => return result,
        }
    }
}

#[tokio::test]
async fn screenshot2_needs_the_desktop_file_and_returns_the_output_size() {
    if std::env::var_os("STILLWATCH_KWIN_HARNESS").is_none() {
        eprintln!("skipping: STILLWATCH_KWIN_HARNESS is unset");
        return;
    }
    let applications = PathBuf::from(std::env::var_os("STILLWATCH_KWIN_APPLICATIONS_DIR").unwrap());
    std::fs::create_dir_all(&applications).unwrap();

    let capture = KwinCapture::connect().await.unwrap();
    let outputs = capture.outputs().await.unwrap();
    let output = outputs.first().unwrap().clone();
    if let Some(size) = expected_size() {
        assert_eq!((output.width, output.height), size, "{output:?}");
    }

    let err = capture.capture(&output.name, 480).await.unwrap_err();
    assert!(matches!(err, BackendError::PermissionDenied(_)), "{err}");

    let _desktop = DesktopFile::install(&applications).unwrap();
    let result = capture_once_authorized(&capture, &output.name)
        .await
        .unwrap();
    assert_eq!(
        (result.meta.width, result.meta.height),
        (output.width, output.height)
    );
    assert_eq!(result.grid.width(), output.width.min(480));
}
