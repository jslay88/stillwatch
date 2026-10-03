//! `KwinCapture` against the shared headless `KWin` harness, including
//! `.desktop` authorization. An authorized caller gets a frame whose size
//! matches `KwinOptions`. That needs OpenGL compositing (`vgem` plus llvmpipe
//! when the machine has no GPU).

use std::time::Duration;

use stillwatch_core::backend::{BackendError, ScreenCapture as _};
use stillwatch_testkit::kwin::{Authorization, Kwin, KwinOptions, SCREENSHOT2};
use stillwatchd::capture::kwin::KwinCapture;
use stillwatchd::outputs;

const NAME_TIMEOUT: Duration = Duration::from_secs(10);

#[tokio::test]
async fn screenshot2_needs_the_desktop_file_and_returns_the_output_size() {
    let Some(denied) = Kwin::start(KwinOptions::default()).await.unwrap() else {
        return;
    };
    denied
        .wait_for_name(SCREENSHOT2, NAME_TIMEOUT)
        .await
        .unwrap();
    let bus = denied.bus().connect().await.unwrap();
    let capture = KwinCapture::on(&bus).await.unwrap();
    let err = capture.capture("Virtual-0", 480).await.unwrap_err();
    assert!(matches!(err, BackendError::PermissionDenied(_)), "{err}");
    drop(denied);

    let width = 1920u32;
    let height = 1080u32;
    let Some(kwin) = Kwin::start(KwinOptions {
        width,
        height,
        authorize: vec![Authorization::current_exe(&[SCREENSHOT2]).unwrap()],
        ..KwinOptions::default()
    })
    .await
    .unwrap() else {
        return;
    };
    kwin.wait_for_name(SCREENSHOT2, NAME_TIMEOUT).await.unwrap();
    let wayland = kwin.connect().unwrap();
    let bus = kwin.bus().connect().await.unwrap();
    let capture = KwinCapture::on(&bus).await.unwrap().with_outputs(move || {
        let wayland = wayland.clone();
        Box::pin(async move { outputs::list_on(&wayland).await })
    });
    let outputs = capture.outputs().await.unwrap();
    let output = outputs.first().expect("Virtual-0");
    assert_eq!(output.name, "Virtual-0");
    assert_eq!((output.width, output.height), (width, height));

    let result = capture.capture(&output.name, 480).await.unwrap();
    assert_eq!((result.meta.width, result.meta.height), (width, height));
    assert_eq!(result.grid.width(), width.min(480));
}
