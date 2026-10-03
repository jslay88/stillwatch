//! The `ScreenCast` session handshake: `CreateSession`, `SelectSources`, `Start`.
//!
//! `SelectSources` asks for every monitor (`multiple = true`) and a persistent
//! restore token. `persist_mode` is the portal's persistent value (2). `ashpd`
//! names that variant `ExplicitlyRevoked`: the grant lasts until the user
//! revokes it. This does not open a dialog by itself; `Start` does, on a bus
//! that has a real portal. Tests pass a private bus with a fake portal.

use std::os::fd::OwnedFd;

use ashpd::desktop::screencast::{
    CursorMode, OpenPipeWireRemoteOptions, Screencast, SelectSourcesOptions, SourceType,
    StartCastOptions,
};
use ashpd::desktop::{PersistMode, Session};
use ashpd::enumflags2::BitFlags;
use stillwatch_core::backend::BackendError;

use super::error::map_ashpd;
use super::map::StreamGeom;

/// A portal session and the `PipeWire` remote it handed back.
pub struct Opened {
    /// Streams the portal selected. No pixels.
    pub streams: Vec<StreamGeom>,
    /// The restore token from this `Start`, when the portal sent one.
    pub restore_token: Option<String>,
    /// File descriptor of the `PipeWire` remote.
    pub remote: OwnedFd,
    /// The portal session. Closing it drops the screen-sharing indicator.
    pub(crate) session: Session<Screencast>,
}

impl Opened {
    /// Ends the portal session.
    ///
    /// # Errors
    ///
    /// [`BackendError`] when the portal doesn't answer `Close`.
    pub async fn close(self) -> Result<(), BackendError> {
        self.session.close().await.map_err(map_ashpd)
    }
}

/// Opens a screen cast of all monitors on `conn`.
///
/// `restore` is the token from the previous `Start`, if there is one. The
/// caller stores [`Opened::restore_token`] when it is `Some`.
///
/// # Errors
///
/// [`BackendError::Unavailable`] when the portal isn't on `conn`,
/// [`BackendError::PermissionDenied`] when the user cancels, and
/// [`BackendError::Disconnected`] or [`BackendError::Protocol`] otherwise.
pub async fn open_session(
    conn: &zbus::Connection,
    restore: Option<&str>,
) -> Result<Opened, BackendError> {
    let proxy = Screencast::with_connection(conn.clone())
        .await
        .map_err(map_ashpd)?;
    let session = proxy
        .create_session(ashpd::desktop::CreateSessionOptions::default())
        .await
        .map_err(map_ashpd)?;
    let mut sources = SelectSourcesOptions::default()
        .set_cursor_mode(CursorMode::Hidden)
        .set_sources(BitFlags::from(SourceType::Monitor))
        .set_multiple(true)
        .set_persist_mode(PersistMode::ExplicitlyRevoked);
    if let Some(token) = restore {
        sources = sources.set_restore_token(Some(token));
    }
    proxy
        .select_sources(&session, sources)
        .await
        .map_err(map_ashpd)?
        .response()
        .map_err(map_ashpd)?;
    let started = proxy
        .start(&session, None, StartCastOptions::default())
        .await
        .map_err(map_ashpd)?
        .response()
        .map_err(map_ashpd)?;
    let streams = started
        .streams()
        .iter()
        .map(|stream| StreamGeom {
            node_id: stream.pipe_wire_node_id(),
            position: stream.position(),
            size: stream.size(),
        })
        .collect();
    let remote = proxy
        .open_pipe_wire_remote(&session, OpenPipeWireRemoteOptions::default())
        .await
        .map_err(map_ashpd)?;
    tracing::debug!(
        streams = started.streams().len(),
        restored = restore.is_some(),
        refreshed = started.restore_token().is_some(),
        "portal screen cast started"
    );
    Ok(Opened {
        streams,
        restore_token: started.restore_token().map(str::to_owned),
        remote,
        session,
    })
}
