//! `OverlayBlanker` against a private `kwin_wayland --virtual`: overlays
//! show on the virtual outputs, follow hotplug, and report when they go away.
//! Skips without `KWin` unless `STILLWATCH_REQUIRE_KWIN=1`.

#[cfg(test)]
mod support;

use stillwatch_core::backend::{BackendError, Blanker, Dimmer};
use support::kwin::Kwin;
use support::overlay::{WAIT, names, power, start};
use tokio::time::timeout;

#[tokio::test]
async fn blank_dim_and_unblank_on_every_output() {
    let Some(kwin) = Kwin::start(1) else { return };
    let mut run = start(&kwin);

    run.blanker.blank(&[]).await.unwrap();
    assert_eq!(run.next().await, power("Virtual-0", false));

    run.blanker.dim(&[], 20).await.unwrap();
    run.blanker.undim(&[]).await.unwrap();
    run.quiet().await;

    run.blanker.dim(&names(&["Virtual-0"]), 20).await.unwrap();
    assert_eq!(run.next().await, power("Virtual-0", false));
    run.blanker.blank(&names(&["Virtual-0"])).await.unwrap();
    run.blanker.undim(&[]).await.unwrap();
    run.blanker.unblank(&[]).await.unwrap();
    run.quiet().await;
    assert!(!run.watch.is_finished());
}

/// Prints every window as `class layer WxH+X+Y`.
const STACK: &str = "for (const w of workspace.stackingOrder) { \
    const g = w.frameGeometry; \
    print(w.resourceClass + ' ' + w.layer + ' ' + g.width + 'x' + g.height + '+' + g.x + '+' + g.y); }";

#[tokio::test]
async fn overlay_covers_the_output_on_kwins_overlay_layer() {
    let Some(kwin) = Kwin::start(1) else { return };
    let mut run = start(&kwin);
    run.blanker.blank(&[]).await.unwrap();
    assert_eq!(run.next().await, power("Virtual-0", false));

    let Some(windows) = kwin.script(STACK, 1) else {
        return;
    };
    let [window] = windows.as_slice() else {
        panic!("{windows:?}")
    };
    let (class, placement) = window.split_once(' ').unwrap();
    assert!(class.starts_with("overlay_kwin"), "{window}");
    // `KWin`'s OverlayLayer (9) is above ActiveLayer (5), where a focused
    // fullscreen window goes.
    assert_eq!(placement, "9 1920x1080+0+0");
}

#[tokio::test]
async fn compositor_closing_the_overlay_reports_it_gone() {
    let Some(kwin) = Kwin::start(1) else { return };
    let mut run = start(&kwin);
    run.blanker.blank(&[]).await.unwrap();
    assert_eq!(run.next().await, power("Virtual-0", false));

    let close = "for (const w of workspace.stackingOrder) { w.closeWindow(); print('closed'); }";
    if kwin.script(close, 1).is_none() {
        return;
    }
    assert_eq!(run.next().await, power("Virtual-0", true));

    run.blanker.blank(&[]).await.unwrap();
    assert_eq!(run.next().await, power("Virtual-0", false));
}

#[tokio::test]
async fn named_targets_only_cover_those_outputs() {
    let Some(kwin) = Kwin::start(2) else { return };
    let mut run = start(&kwin);

    let error = run.blanker.blank(&names(&["HDMI-A-9"])).await.unwrap_err();
    assert!(matches!(error, BackendError::NotFound(_)), "{error}");

    run.blanker
        .blank(&names(&["Virtual-1", "HDMI-A-9"]))
        .await
        .unwrap();
    assert_eq!(run.next().await, power("Virtual-1", false));
    run.quiet().await;

    run.blanker.unblank(&names(&["Virtual-1"])).await.unwrap();
    run.quiet().await;
}

#[tokio::test]
async fn removed_output_reports_the_overlay_gone_and_hotplug_restores_it() {
    let Some(kwin) = Kwin::start(2) else { return };
    let mut run = start(&kwin);
    run.blanker.blank(&[]).await.unwrap();
    assert_eq!(
        run.next_n(2).await,
        [power("Virtual-0", false), power("Virtual-1", false)]
    );

    if kwin.kscreen_doctor(&["output.Virtual-1.disable"]).is_none() {
        return;
    }
    assert_eq!(run.next().await, power("Virtual-1", true));

    kwin.kscreen_doctor(&["output.Virtual-1.enable"]);
    assert_eq!(run.next().await, power("Virtual-1", false));

    run.blanker.unblank(&[]).await.unwrap();
    kwin.kscreen_doctor(&["output.Virtual-1.disable"]);
    run.quiet().await;
}

#[tokio::test]
async fn losing_the_compositor_reports_every_overlay_gone() {
    let Some(mut kwin) = Kwin::start(1) else {
        return;
    };
    let mut run = start(&kwin);
    run.blanker.blank(&[]).await.unwrap();
    assert_eq!(run.next().await, power("Virtual-0", false));

    kwin.kill();
    assert_eq!(run.next().await, power("Virtual-0", true));
    let ended = timeout(WAIT, run.watch).await.unwrap().unwrap();
    assert!(
        matches!(ended, Err(BackendError::Disconnected(_))),
        "{ended:?}"
    );

    let error = run.blanker.blank(&[]).await.unwrap_err();
    assert!(error.is_transient(), "{error}");
    run.blanker.unblank(&[]).await.unwrap();
}
