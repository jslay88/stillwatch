use ashpd::desktop::ResponseError;
use ashpd::{Error, PortalError};
use stillwatch_core::backend::BackendError;
use zbus::names::OwnedInterfaceName;

use super::{blocks_capture, map_ashpd, map_pipewire};

fn iface() -> OwnedInterfaceName {
    OwnedInterfaceName::try_from("org.freedesktop.portal.ScreenCast").unwrap()
}

#[test]
fn denial_and_a_missing_portal_block_capture() {
    let denied = map_ashpd(Error::Response(ResponseError::Cancelled));
    assert!(matches!(denied, BackendError::PermissionDenied(_)));
    assert!(blocks_capture(&denied));

    let dismissed = map_ashpd(Error::Response(ResponseError::Other));
    assert!(blocks_capture(&dismissed));

    let missing = map_ashpd(Error::PortalNotFound(iface()));
    assert!(matches!(missing, BackendError::Unavailable(_)));
    assert!(blocks_capture(&missing));
    assert!(missing.to_string().contains("ScreenCast"));
}

#[test]
fn a_dropped_request_and_pipewire_stay_retryable() {
    let dropped = map_ashpd(Error::NoResponse);
    assert!(dropped.is_transient());
    assert!(!blocks_capture(&dropped));

    let pipe = map_pipewire(&pipewire::Error::CreationFailed);
    assert!(matches!(pipe, BackendError::Disconnected(_)));
    assert!(pipe.is_transient());
}

#[test]
fn portal_method_errors_keep_their_kind() {
    let cancelled = map_ashpd(Error::Portal(PortalError::Cancelled("no".into())));
    assert!(matches!(cancelled, BackendError::PermissionDenied(_)));

    let refused = map_ashpd(Error::Portal(PortalError::NotAllowed("no".into())));
    assert!(matches!(refused, BackendError::PermissionDenied(_)));

    let gone = map_ashpd(Error::Portal(PortalError::NotFound("session".into())));
    assert_eq!(gone, BackendError::NotFound("session".into()));

    let version = map_ashpd(Error::RequiresVersion(5, 2));
    assert!(matches!(version, BackendError::Unsupported(_)));
    assert!(!blocks_capture(&version));
}
