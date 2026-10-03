use super::*;

const MISSING: &str = "stillwatch-testkit-no-such-kwin";

fn quick() -> KwinOptions {
    KwinOptions {
        ready_timeout: Duration::from_millis(300),
        ..KwinOptions::default()
    }
}

fn have_dbus() -> bool {
    matches!(PrivateBus::start(), Ok(Some(_)))
}

#[test]
fn arguments_pin_the_virtual_outputs() {
    let options = KwinOptions {
        width: 640,
        height: 480,
        outputs: 2,
        ..KwinOptions::default()
    };
    assert_eq!(
        arguments(&options).join(" "),
        "--virtual --no-lockscreen --no-global-shortcuts --no-kactivities \
         --socket wayland-stillwatch --width 640 --height 480 --output-count 2"
    );
}

#[tokio::test]
async fn a_missing_kwin_skips_unless_required() {
    if !have_dbus() {
        return;
    }
    let skipped = Kwin::launch(MISSING, &[], &quick(), false).await.unwrap();
    assert!(skipped.is_none());
    let err = Kwin::launch(MISSING, &[], &quick(), true)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::KwinMissing(ref name) if name == MISSING));
    assert!(err.to_string().contains(REQUIRE_ENV), "{err}");
}

#[tokio::test]
async fn exiting_during_startup_reports_the_log() {
    if !have_dbus() {
        return;
    }
    let args = ["-c".to_owned(), "echo kwin went away; exit 3".to_owned()];
    let err = Kwin::launch("sh", &args, &quick(), true).await.unwrap_err();
    let Error::KwinExited { status, log } = err else {
        panic!("{err}");
    };
    assert_eq!(status.code(), Some(3));
    assert_eq!(log, "kwin went away");
}

#[tokio::test]
async fn no_socket_times_out() {
    if !have_dbus() {
        return;
    }
    let args = ["600".to_owned()];
    let err = Kwin::launch("sleep", &args, &quick(), true)
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::Timeout { ref what, .. } if what == "the Wayland socket"),
        "{err}"
    );
}

#[tokio::test]
async fn an_unquotable_grant_fails_before_anything_starts() {
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("needs quoting");
    std::fs::write(&exe, "").unwrap();
    let options = KwinOptions {
        authorize: vec![Authorization::new(exe, &[SCREENSHOT2])],
        ..quick()
    };
    let err = Kwin::launch(MISSING, &[], &options, true)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::UnquotablePath(_)), "{err}");
}
