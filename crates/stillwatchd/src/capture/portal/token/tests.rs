use std::fs;
use std::os::unix::fs::PermissionsExt as _;

use stillwatch_core::backend::BackendError;

use super::TokenStore;

#[test]
fn missing_file_loads_as_absent() {
    let dir = tempfile::tempdir().unwrap();
    let store = TokenStore::in_dir(dir.path());
    assert_eq!(store.load().unwrap(), None);
}

#[test]
fn round_trip_and_refresh() {
    let dir = tempfile::tempdir().unwrap();
    let store = TokenStore::in_dir(dir.path());
    store.store(Some("first-grant")).unwrap();
    assert_eq!(store.load().unwrap().as_deref(), Some("first-grant"));

    let mode = fs::metadata(store.path()).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);

    store.store(Some("  second-grant\n")).unwrap();
    assert_eq!(store.load().unwrap().as_deref(), Some("second-grant"));
}

#[test]
fn empty_response_keeps_the_previous_token() {
    let dir = tempfile::tempdir().unwrap();
    let store = TokenStore::in_dir(dir.path());
    store.store(Some("kept")).unwrap();
    store.store(None).unwrap();
    store.store(Some("   ")).unwrap();
    assert_eq!(store.load().unwrap().as_deref(), Some("kept"));
}

#[test]
fn a_newline_inside_the_token_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let store = TokenStore::in_dir(dir.path());
    let err = store.store(Some("one\ntwo")).unwrap_err();
    assert!(matches!(err, BackendError::Protocol(_)));
    assert_eq!(store.load().unwrap(), None);
}

#[test]
fn an_empty_file_is_absent() {
    let dir = tempfile::tempdir().unwrap();
    let store = TokenStore::in_dir(dir.path());
    fs::write(store.path(), "\n").unwrap();
    assert_eq!(store.load().unwrap(), None);
}
