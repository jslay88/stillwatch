//! `outputs::list` against whatever compositor `WAYLAND_DISPLAY` points at
//! (the desktop, or `kwin_wayland --virtual` in CI). Without one, only the
//! "no compositor" path is checked.

use stillwatchd::outputs;

#[tokio::test]
async fn lists_named_outputs_or_reports_no_compositor() {
    let result = outputs::list().await;
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        let error = result.unwrap_err();
        assert!(error.is_transient(), "{error}");
        return;
    }
    let outputs = result.unwrap();
    assert!(!outputs.is_empty(), "the compositor reported no outputs");
    for output in &outputs {
        assert!(!output.name.is_empty(), "{output:?}");
        assert!(output.width > 0 && output.height > 0, "{output:?}");
    }
}
